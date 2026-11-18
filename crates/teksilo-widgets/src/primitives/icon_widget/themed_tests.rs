// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`IconWidget::themed`] over a fixture icon theme held in memory.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use teksilo_canvas::{RenderFrame, SizeProposal};
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::icon_theme::IconThemeReader;
use teksilo_tokens::Color;

use super::{IconMode, IconThemeService, IconWidget};

/// Files in memory, shared with the service so a test can install an icon
/// after the service was registered.
#[derive(Clone, Default)]
struct Files(Rc<RefCell<HashMap<PathBuf, Vec<u8>>>>);

impl Files {
    fn add(&self, path: &str, bytes: &[u8]) {
        self.0
            .borrow_mut()
            .insert(PathBuf::from(path), bytes.to_vec());
    }
}

impl IconThemeReader for Files {
    fn read(&self, path: &Path, _limit: u64) -> Option<Vec<u8>> {
        self.0.borrow().get(path).cloned()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.0.borrow().contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.0
            .borrow()
            .keys()
            .any(|file| file != path && file.starts_with(path))
    }
}

/// A 2×2 RGBA PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d,
    0x24, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x91, 0x62, 0xf4,
    0x1f, 0x84, 0x19, 0x60, 0x0c, 0x00, 0x54, 0x40, 0x09, 0x75, 0xa6, 0x68, 0x90, 0xc2, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A symbolic glyph as themes draw them, in a fixed grey.
const SYMBOLIC_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
    <path fill="#2e3436" d="M2 5l6 6 6-6z"/>
</svg>"##;

const THEME: &str = "/usr/share/icons/fixture";

/// The `fixture` theme: a 16 px directory with a symbolic SVG, a symbolic
/// PNG and a full-colour PNG.
fn fixture() -> Files {
    let files = Files::default();
    files.add(
        &format!("{THEME}/index.theme"),
        b"[Icon Theme]\nName=Fixture\nDirectories=16\n\n[16]\nSize=16\nType=Threshold\n",
    );
    files.add(
        &format!("{THEME}/16/go-down-symbolic.svg"),
        SYMBOLIC_SVG.as_bytes(),
    );
    files.add(&format!("{THEME}/16/view-refresh-symbolic.png"), PNG);
    files.add(&format!("{THEME}/16/face-smile.png"), PNG);
    files
}

fn service(files: &Files) -> IconThemeService {
    IconThemeService::with_source(
        files.clone(),
        |name| (name == "HOME").then(|| "/home/ada".to_owned()),
        Some("fixture".to_owned()),
    )
}

/// A tree whose application has `service`, or none.
fn tree(service: Option<IconThemeService>) -> WidgetTree {
    let mut tree = WidgetTree::new();
    if let Some(service) = service {
        let mut registry: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        registry.insert(TypeId::of::<IconThemeService>(), Box::new(service));
        tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));
    }
    tree
}

fn render(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::unspecified());
    tree.render()
}

#[test]
fn a_symbolic_svg_from_the_theme_is_tinted() {
    let mut tree = tree(Some(service(&fixture())));
    let icon = tree.add(IconWidget::themed("go-down-symbolic").color(Color::BLACK));
    let frame = render(&mut tree);
    assert!(!frame.paths.is_empty(), "the theme's glyph is drawn");
    assert!(
        frame
            .paths
            .iter()
            .all(|p| p.color == Color::BLACK.to_array()),
        "in the widget's colour, not the file's grey"
    );
    let bounds = tree.bounds(icon);
    assert_eq!((bounds.width, bounds.height), (16.0, 16.0));
}

#[test]
fn a_symbolic_png_is_tinted_and_a_full_colour_one_is_not() {
    let mut tree = tree(Some(service(&fixture())));
    tree.add(IconWidget::themed("view-refresh-symbolic").color(Color::BLACK));
    let frame = render(&mut tree);
    assert_eq!(frame.images.len(), 1);
    assert!(frame.images[0].tint.is_some(), "symbolic: tinted");

    let mut tree = self::tree(Some(service(&fixture())));
    tree.add(IconWidget::themed("face-smile").color(Color::BLACK));
    let frame = render(&mut tree);
    assert_eq!(frame.images.len(), 1);
    assert!(
        frame.images[0].tint.is_none(),
        "full colour: its own pixels"
    );
}

#[test]
fn one_icon_in_two_widgets_is_decoded_once() {
    let mut tree = tree(Some(service(&fixture())));
    tree.add(IconWidget::themed("face-smile"));
    tree.add(IconWidget::themed("face-smile"));
    let frame = render(&mut tree);
    let names: std::collections::BTreeSet<&str> =
        frame.images.iter().map(|q| q.name.as_str()).collect();
    assert_eq!(frame.images.len(), 2);
    assert_eq!(names.len(), 1, "one texture: {names:?}");
}

#[test]
fn a_missing_name_draws_the_fallback_at_the_themed_size() {
    let mut tree = tree(Some(service(&fixture())));
    let icon = tree.add(
        IconWidget::themed("no-such-icon")
            .fallback(IconWidget::checkmark(24.0).mode(IconMode::FullColor))
            .icon_size(20.0)
            .color(Color::BLACK),
    );
    let frame = render(&mut tree);
    assert!(!frame.paths.is_empty(), "the fallback's checkmark is drawn");
    let bounds = tree.bounds(icon);
    assert_eq!((bounds.width, bounds.height), (20.0, 20.0));
}

#[test]
fn a_missing_name_without_fallback_draws_nothing_in_its_space() {
    let mut tree = tree(Some(service(&fixture())));
    let icon = tree.add(IconWidget::themed("no-such-icon").color(Color::BLACK));
    let frame = render(&mut tree);
    assert!(frame.paths.is_empty() && frame.images.is_empty());
    let bounds = tree.bounds(icon);
    assert_eq!((bounds.width, bounds.height), (16.0, 16.0));
}

#[test]
fn without_an_icon_theme_service_the_fallback_is_drawn() {
    let mut tree = tree(None);
    tree.add(
        IconWidget::themed("face-smile")
            .fallback(IconWidget::checkmark(16.0))
            .color(Color::BLACK),
    );
    let frame = render(&mut tree);
    assert!(frame.images.is_empty());
    assert!(!frame.paths.is_empty());
}

#[test]
fn a_reload_finds_an_icon_installed_since() {
    let files = fixture();
    let mut tree = tree(Some(service(&files)));
    tree.add(
        IconWidget::themed("mail-unread")
            .fallback(IconWidget::checkmark(16.0))
            .color(Color::BLACK),
    );
    let frame = render(&mut tree);
    assert!(frame.images.is_empty(), "not installed yet: the fallback");

    files.add(&format!("{THEME}/16/mail-unread.png"), PNG);
    let frame = render(&mut tree);
    assert!(
        frame.images.is_empty(),
        "a miss is remembered until a reload"
    );

    tree.app_context()
        .app_state::<IconThemeService>()
        .unwrap()
        .reload();
    let frame = render(&mut tree);
    assert_eq!(frame.images.len(), 1, "rebuilt with the theme's icon");
}
