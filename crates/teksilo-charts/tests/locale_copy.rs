// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The chart messages live twice: in `locales/en-US.ftl` here, which
//! `tr_widget!` checks keys against and falls back on with no `I18nManager`,
//! and in teksilo-widgets' `locales/en-US.ftl`, the bundle every application
//! registers, which is what a running application reads (with every
//! translation beside it). The two copies must say the same thing.

use std::collections::BTreeMap;

/// Each message of an `.ftl` source, by id, as its exact source text.
fn messages(source: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in source.lines() {
        let continues = line.starts_with(' ') && current.is_some();
        if continues {
            if let Some((_, text)) = current.as_mut() {
                text.push('\n');
                text.push_str(line);
            }
            continue;
        }
        if let Some((id, text)) = current.take() {
            out.insert(id, text);
        }
        if let Some((id, rest)) = line.split_once(" =")
            && !line.starts_with('#')
            && !id.is_empty()
            && !id.contains(' ')
        {
            current = Some((id.to_string(), rest.to_string()));
        }
    }
    if let Some((id, text)) = current.take() {
        out.insert(id, text);
    }
    out
}

#[test]
fn every_chart_message_is_the_one_applications_read() {
    let here = concat!(env!("CARGO_MANIFEST_DIR"), "/locales/en-US.ftl");
    let widgets = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../teksilo-widgets/locales/en-US.ftl"
    );
    let ours = messages(&std::fs::read_to_string(here).expect("charts' en-US.ftl"));
    let theirs = messages(&std::fs::read_to_string(widgets).expect("widgets' en-US.ftl"));
    assert!(!ours.is_empty(), "the charts' catalogue has messages");
    for (id, text) in &ours {
        assert_eq!(
            theirs.get(id),
            Some(text),
            "{id} differs from, or is missing in, teksilo-widgets/locales/en-US.ftl"
        );
    }
}
