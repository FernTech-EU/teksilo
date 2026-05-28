//! Minimal read-only XML DOM over `quick-xml`'s event stream.
//!
//! The SVG icon parser ([`crate::svg`]) recurses over a tree and reads
//! attributes by name — a workflow that fits a DOM model. `quick-xml`
//! itself is event-streaming, so this module builds a lightweight tree
//! (`XmlElement`) from the event sequence once, and the rest of the
//! parser walks it the same way it used to walk `roxmltree::Node`s.
//!
//! Only elements + attributes are kept; text, comments, processing
//! instructions, and doctypes are dropped because the SVG icon parser
//! never reads them. Namespaces are flattened to local names because
//! SVG uses one default namespace and the parser only matches against
//! the bare element name.

use std::collections::HashMap;

/// One parsed XML element with its attributes and child elements.
#[derive(Debug, Clone)]
pub(crate) struct XmlElement {
    pub(crate) name: String,
    pub(crate) attrs: HashMap<String, String>,
    pub(crate) children: Vec<XmlElement>,
}

impl XmlElement {
    /// Local tag name (no namespace prefix).
    pub(crate) fn tag_name(&self) -> &str {
        &self.name
    }

    /// Look up an attribute by local name. Returns `None` if absent.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    /// Iterate over child elements (text/comment nodes are not retained).
    pub(crate) fn children(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter()
    }
}

/// Parse an XML document into a single root [`XmlElement`].
///
/// Returns `Ok(None)` for a valid but empty document (no element ever
/// seen). XML parse errors are bubbled up as `Err(String)`.
pub(crate) fn parse_dom(text: &str) -> Result<Option<XmlElement>, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(text);
    let mut buf: Vec<u8> = Vec::new();
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let elem = element_from_start(&e)?;
                stack.push(elem);
            }
            Ok(Event::Empty(e)) => {
                // Self-closing tag: build a leaf, attach to parent (or
                // set as root if at top level).
                let elem = element_from_start(&e)?;
                attach_or_root(&mut stack, &mut root, elem);
            }
            Ok(Event::End(_)) => {
                let elem = stack
                    .pop()
                    .ok_or_else(|| "unmatched end tag".to_string())?;
                attach_or_root(&mut stack, &mut root, elem);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {
                // Text, CData, Comment, PI, Decl, DocType — not retained.
            }
            Err(e) => return Err(e.to_string()),
        }
        buf.clear();
    }

    if !stack.is_empty() {
        return Err("unclosed elements at EOF".to_string());
    }
    Ok(root)
}

fn attach_or_root(
    stack: &mut [XmlElement],
    root: &mut Option<XmlElement>,
    elem: XmlElement,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(elem);
    } else {
        // First top-level element wins; subsequent ones at the document
        // level are ignored (XML 1.0 allows only one root element, but
        // we tolerate extras quietly rather than erroring).
        if root.is_none() {
            *root = Some(elem);
        }
    }
}

fn element_from_start(e: &quick_xml::events::BytesStart<'_>) -> Result<XmlElement, String> {
    let name_bytes = e.local_name();
    let name = std::str::from_utf8(name_bytes.as_ref())
        .map_err(|err| format!("non-UTF-8 element name: {err}"))?
        .to_string();

    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| err.to_string())?;
        let key_bytes = attr.key.local_name();
        let key = std::str::from_utf8(key_bytes.as_ref())
            .map_err(|err| format!("non-UTF-8 attribute name: {err}"))?
            .to_string();
        // SVG icon files are XML 1.0 in practice — that's what every
        // editor and asset pipeline targets. `normalized_value` needs
        // an explicit version because XML 1.1 normalises a slightly
        // different set of whitespace characters; `Implicit1_0` matches
        // documents that omit the `<?xml version="1.0" ?>` declaration
        // (the common case for inline SVG icons).
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| err.to_string())?
            .into_owned();
        attrs.insert(key, value);
    }

    Ok(XmlElement {
        name,
        attrs,
        children: Vec::new(),
    })
}
