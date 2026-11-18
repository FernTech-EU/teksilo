// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fixture trees held in memory: no test reads the real file system or the
//! process environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;

/// A file tree in memory. A directory exists when a file lies under it.
#[derive(Default)]
struct Tree(HashMap<PathBuf, Vec<u8>>);

impl Tree {
    fn file(mut self, path: &str, text: &str) -> Tree {
        self.0.insert(PathBuf::from(path), text.as_bytes().to_vec());
        self
    }

    /// An empty icon file.
    fn icon(self, path: &str) -> Tree {
        self.file(path, "")
    }
}

impl IconThemeReader for Tree {
    fn read(&self, path: &Path, _limit: u64) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.0
            .keys()
            .any(|file| file != path && file.starts_with(path))
    }
}

/// An environment getter over a fixed table.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let table: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| table.get(name).cloned()
}

/// User ada, with the default XDG directories.
fn ada() -> impl Fn(&str) -> Option<String> + use<> {
    env(&[("HOME", "/home/ada")])
}

const SHARE: &str = "/usr/share/icons";

/// An `index.theme` with `inherits` and one group per `(dir, body)`.
fn index(inherits: Option<&str>, dirs: &[(&str, &str)]) -> String {
    let mut text = String::from("[Icon Theme]\nName=Test\nComment=A theme\n");
    if let Some(inherits) = inherits {
        text.push_str(&format!("Inherits={inherits}\n"));
    }
    let names: Vec<&str> = dirs.iter().map(|(dir, _)| *dir).collect();
    text.push_str(&format!("Directories={}\n", names.join(",")));
    for (dir, body) in dirs {
        text.push_str(&format!("\n[{dir}]\n{body}\n"));
    }
    text
}

/// A minimal `hicolor` with one fixed 48 px directory.
fn with_hicolor(tree: Tree) -> Tree {
    tree.file(
        "/usr/share/icons/hicolor/index.theme",
        &index(None, &[("48x48/apps", "Size=48\nType=Fixed")]),
    )
}

fn load(tree: &Tree, name: &str) -> Option<IconTheme> {
    IconTheme::load(name, tree, &ada())
}

fn find(theme: &IconTheme, tree: &Tree, request: IconRequest<'_>) -> Option<String> {
    theme
        .lookup(tree, &request)
        .map(|file| file.path.to_string_lossy().replace('\\', "/"))
}

// ---- Base directories --------------------------------------------------

#[test]
fn the_base_directories_follow_the_xdg_variables() {
    let paths = |dirs: Vec<PathBuf>| -> Vec<String> {
        dirs.iter()
            .map(|d| d.to_string_lossy().replace('\\', "/"))
            .collect()
    };
    assert_eq!(
        paths(base_directories(&ada())),
        [
            "/home/ada/.icons",
            "/home/ada/.local/share/icons",
            "/usr/local/share/icons",
            "/usr/share/icons",
            "/usr/share/pixmaps",
        ]
    );
    let custom = env(&[
        ("HOME", "/home/ada"),
        ("XDG_DATA_HOME", "/data"),
        // A relative entry is ignored, a repeated one kept once.
        (
            "XDG_DATA_DIRS",
            "/opt/share:relative/share:/usr/share:/opt/share",
        ),
    ]);
    assert_eq!(
        paths(base_directories(&custom)),
        [
            "/home/ada/.icons",
            "/data/icons",
            "/opt/share/icons",
            "/usr/share/icons",
            "/usr/share/pixmaps",
        ]
    );
    // No home: only the system directories.
    let homeless = env(&[("XDG_DATA_HOME", "relative")]);
    assert_eq!(
        paths(base_directories(&homeless)),
        [
            "/usr/local/share/icons",
            "/usr/share/icons",
            "/usr/share/pixmaps"
        ]
    );
}

// ---- index.theme --------------------------------------------------------

#[test]
fn an_index_is_read_as_a_desktop_entry_file() {
    let parsed = parse_index(
        "# comment\n[Icon Theme]\nName = Breeze\nName[fr]=Brise\nName=Second\n\n\
         [16x16/actions]\nSize=16\n[Icon Theme]\nName=Again\n",
    );
    let theme = &parsed["Icon Theme"];
    assert_eq!(theme["Name"], "Breeze");
    assert_eq!(theme["Name[fr]"], "Brise");
    assert_eq!(parsed["16x16/actions"]["Size"], "16");
}

#[test]
fn a_directory_without_a_size_is_ignored_and_the_defaults_fill_the_rest() {
    let tree = with_hicolor(Tree::default().file(
        "/usr/share/icons/t/index.theme",
        &index(None, &[("nosize", "Type=Fixed"), ("22", "Size=22")]),
    ));
    let theme = load(&tree, "t").unwrap();
    let dirs = &theme.chain[0].directories;
    assert_eq!(dirs.len(), 1);
    let dir = &dirs[0];
    assert_eq!(dir.kind, DirectoryType::Threshold);
    assert_eq!(
        (dir.scale, dir.min_size, dir.max_size, dir.threshold),
        (1, 22, 22, 2)
    );
}

// ---- Size matching --------------------------------------------------------

#[test]
fn a_matching_directory_wins_over_an_earlier_closer_one() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                "/usr/share/icons/t/index.theme",
                &index(
                    None,
                    &[
                        ("16", "Size=16\nType=Fixed"),
                        ("22", "Size=22\nType=Fixed"),
                        ("32", "Size=32\nType=Fixed"),
                    ],
                ),
            )
            .icon("/usr/share/icons/t/16/go-down.svg")
            .icon("/usr/share/icons/t/22/go-down.svg")
            .icon("/usr/share/icons/t/32/go-down.svg"),
    );
    let theme = load(&tree, "t").unwrap();
    let at = |size| find(&theme, &tree, IconRequest::new("go-down", size));
    assert_eq!(at(22).as_deref(), Some("/usr/share/icons/t/22/go-down.svg"));
    assert_eq!(at(32).as_deref(), Some("/usr/share/icons/t/32/go-down.svg"));
    // None matches: the closest; at equal distance the first in order.
    assert_eq!(at(28).as_deref(), Some("/usr/share/icons/t/32/go-down.svg"));
    assert_eq!(at(19).as_deref(), Some("/usr/share/icons/t/16/go-down.svg"));
    assert_eq!(at(64).as_deref(), Some("/usr/share/icons/t/32/go-down.svg"));
}

#[test]
fn threshold_and_scalable_directories_match_a_range() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                "/usr/share/icons/t/index.theme",
                &index(
                    None,
                    &[
                        ("24", "Size=24\nType=Threshold\nThreshold=4"),
                        ("scalable", "Size=16\nType=Scalable\nMinSize=8\nMaxSize=512"),
                    ],
                ),
            )
            .icon("/usr/share/icons/t/24/a.png")
            .icon("/usr/share/icons/t/scalable/a.svg"),
    );
    let theme = load(&tree, "t").unwrap();
    let at = |size| find(&theme, &tree, IconRequest::new("a", size));
    assert_eq!(at(20).as_deref(), Some("/usr/share/icons/t/24/a.png"));
    assert_eq!(at(28).as_deref(), Some("/usr/share/icons/t/24/a.png"));
    assert_eq!(at(29).as_deref(), Some("/usr/share/icons/t/scalable/a.svg"));
    assert_eq!(
        at(256).as_deref(),
        Some("/usr/share/icons/t/scalable/a.svg")
    );
    // Below both: the scalable one is 8 - 4 = 4 away, the threshold one
    // (MinSize defaulting to Size) 24 - 4 = 20.
    assert_eq!(at(4).as_deref(), Some("/usr/share/icons/t/scalable/a.svg"));
}

#[test]
fn png_is_tried_before_svg_in_one_directory() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                "/usr/share/icons/t/index.theme",
                &index(None, &[("48", "Size=48")]),
            )
            .icon("/usr/share/icons/t/48/a.svg")
            .icon("/usr/share/icons/t/48/a.png")
            .icon("/usr/share/icons/t/48/b.xpm"),
    );
    let theme = load(&tree, "t").unwrap();
    let file = theme.lookup(&tree, &IconRequest::new("a", 48)).unwrap();
    assert_eq!(file.kind, IconFileKind::Png);
    assert!(!file.symbolic);
    assert_eq!(theme.lookup(&tree, &IconRequest::new("b", 48)), None);
}

#[test]
fn a_scaled_directory_serves_its_scale_only() {
    let text = "[Icon Theme]\nName=t\nDirectories=22\nScaledDirectories=22@2x\n\n\
                [22]\nSize=22\nType=Fixed\n\n[22@2x]\nSize=22\nScale=2\nType=Fixed\n";
    let tree = with_hicolor(
        Tree::default()
            .file("/usr/share/icons/t/index.theme", text)
            .icon("/usr/share/icons/t/22/a.png")
            .icon("/usr/share/icons/t/22@2x/a.png"),
    );
    let theme = load(&tree, "t").unwrap();
    assert_eq!(
        find(&theme, &tree, IconRequest::new("a", 22)).as_deref(),
        Some("/usr/share/icons/t/22/a.png")
    );
    assert_eq!(
        find(&theme, &tree, IconRequest::new("a", 22).scale(2)).as_deref(),
        Some("/usr/share/icons/t/22@2x/a.png")
    );
    // Scale 2 at 44 px matches neither, and the scaled directory is
    // 44 device pixels, exactly what is wanted.
    assert_eq!(
        find(&theme, &tree, IconRequest::new("a", 44)).as_deref(),
        Some("/usr/share/icons/t/22@2x/a.png")
    );
}

// ---- Inheritance -----------------------------------------------------------

#[test]
fn parents_are_searched_depth_first_and_hicolor_last() {
    let dir = [("48", "Size=48")];
    let tree = with_hicolor(
        Tree::default()
            .file(
                &format!("{SHARE}/a/index.theme"),
                &index(Some("hicolor,b,c"), &dir),
            )
            .file(&format!("{SHARE}/b/index.theme"), &index(Some("d"), &dir))
            .file(&format!("{SHARE}/c/index.theme"), &index(None, &dir))
            .file(&format!("{SHARE}/d/index.theme"), &index(None, &dir))
            .icon("/usr/share/icons/hicolor/48x48/apps/x.png")
            .icon("/usr/share/icons/c/48/x.png")
            .icon("/usr/share/icons/d/48/x.png")
            .icon("/usr/share/icons/hicolor/48x48/apps/only-hicolor.png"),
    );
    let theme = load(&tree, "a").unwrap();
    assert_eq!(theme.name(), "a");
    assert_eq!(
        theme.chain().collect::<Vec<_>>(),
        ["a", "b", "d", "c", "hicolor"]
    );
    assert_eq!(
        find(&theme, &tree, IconRequest::new("x", 48)).as_deref(),
        Some("/usr/share/icons/d/48/x.png")
    );
    assert_eq!(
        find(&theme, &tree, IconRequest::new("only-hicolor", 48)).as_deref(),
        Some("/usr/share/icons/hicolor/48x48/apps/only-hicolor.png")
    );
}

#[test]
fn a_theme_wins_over_its_parent_whatever_the_size() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                &format!("{SHARE}/a/index.theme"),
                &index(None, &[("16", "Size=16\nType=Fixed")]),
            )
            .icon("/usr/share/icons/a/16/x.png")
            .icon("/usr/share/icons/hicolor/48x48/apps/x.png"),
    );
    let theme = load(&tree, "a").unwrap();
    assert_eq!(
        find(&theme, &tree, IconRequest::new("x", 48)).as_deref(),
        Some("/usr/share/icons/a/16/x.png")
    );
}

#[test]
fn an_inheritance_cycle_and_a_missing_parent_are_harmless() {
    let dir = [("48", "Size=48")];
    let tree = with_hicolor(
        Tree::default()
            .file(
                &format!("{SHARE}/a/index.theme"),
                &index(Some("b,missing"), &dir),
            )
            .file(&format!("{SHARE}/b/index.theme"), &index(Some("a"), &dir)),
    );
    let theme = load(&tree, "a").unwrap();
    assert_eq!(theme.chain().collect::<Vec<_>>(), ["a", "b", "hicolor"]);
}

#[test]
fn inheritance_stops_at_its_depth_bound() {
    let dir = [("48", "Size=48")];
    let mut tree = Tree::default();
    for n in 0..40 {
        tree = tree.file(
            &format!("{SHARE}/t{n}/index.theme"),
            &index(Some(&format!("t{}", n + 1)), &dir),
        );
    }
    let theme = load(&with_hicolor(tree), "t0").unwrap();
    let chain: Vec<_> = theme.chain().collect();
    assert_eq!(chain.len(), MAX_INHERITANCE_DEPTH + 2);
    assert_eq!(chain.last(), Some(&"hicolor"));
}

#[test]
fn a_missing_theme_does_not_load_but_hicolor_does() {
    let tree = with_hicolor(Tree::default());
    assert!(load(&tree, "breeze").is_none());
    let hicolor = load(&tree, "hicolor").unwrap();
    assert_eq!(hicolor.chain().collect::<Vec<_>>(), ["hicolor"]);
    assert!(load(&Tree::default(), "hicolor").is_none());
}

#[test]
fn the_first_index_counts_and_every_base_directory_holds_icons() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                "/home/ada/.local/share/icons/t/index.theme",
                &index(None, &[("48", "Size=48")]),
            )
            // A second index further down is not read.
            .file(
                "/usr/share/icons/t/index.theme",
                &index(None, &[("16", "Size=16")]),
            )
            .icon("/usr/share/icons/t/48/x.png")
            .icon("/home/ada/.local/share/icons/hicolor/48x48/apps/user-app.png"),
    );
    let theme = load(&tree, "t").unwrap();
    assert_eq!(theme.chain[0].directories.len(), 1);
    assert_eq!(theme.chain[0].directories[0].size, 48);
    assert_eq!(
        find(&theme, &tree, IconRequest::new("x", 48)).as_deref(),
        Some("/usr/share/icons/t/48/x.png")
    );
    assert_eq!(
        find(&theme, &tree, IconRequest::new("user-app", 48)).as_deref(),
        Some("/home/ada/.local/share/icons/hicolor/48x48/apps/user-app.png")
    );
}

#[test]
fn an_unthemed_icon_is_found_in_the_base_directories_last() {
    let tree = with_hicolor(Tree::default())
        .icon("/usr/share/pixmaps/legacy-app.png")
        .icon("/usr/share/icons/hicolor/48x48/apps/both.png")
        .icon("/usr/share/pixmaps/both.png");
    let theme = load(&tree, "hicolor").unwrap();
    assert_eq!(
        find(&theme, &tree, IconRequest::new("legacy-app", 48)).as_deref(),
        Some("/usr/share/pixmaps/legacy-app.png")
    );
    assert_eq!(
        find(&theme, &tree, IconRequest::new("both", 48)).as_deref(),
        Some("/usr/share/icons/hicolor/48x48/apps/both.png")
    );
}

// ---- Names: symbolic and generic fallback --------------------------------

#[test]
fn a_symbolic_request_prefers_the_symbolic_file() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                &format!("{SHARE}/t/index.theme"),
                &index(None, &[("16", "Size=16")]),
            )
            .icon("/usr/share/icons/t/16/go-down.png")
            .icon("/usr/share/icons/t/16/go-down-symbolic.svg")
            .icon("/usr/share/icons/t/16/view-refresh.svg"),
    );
    let theme = load(&tree, "t").unwrap();

    let symbolic = theme
        .lookup(&tree, &IconRequest::new("go-down", 16).symbolic(true))
        .unwrap();
    assert!(symbolic.symbolic);
    assert_eq!(symbolic.kind, IconFileKind::Svg);
    // The suffix in the name says the same.
    assert_eq!(
        theme.lookup(&tree, &IconRequest::new("go-down-symbolic", 16)),
        Some(symbolic)
    );
    let plain = theme
        .lookup(&tree, &IconRequest::new("go-down", 16))
        .unwrap();
    assert!(!plain.symbolic);
    assert_eq!(plain.kind, IconFileKind::Png);

    // No symbolic variant: the plain icon, said to be plain, unless the
    // generic fallback is off.
    let request = IconRequest::new("view-refresh", 16).symbolic(true);
    let file = theme.lookup(&tree, &request).unwrap();
    assert!(!file.symbolic);
    assert_eq!(
        theme.lookup(&tree, &request.clone().generic_fallback(false)),
        None
    );
}

#[test]
fn the_generic_fallback_drops_trailing_parts_within_each_theme_first() {
    let dir = [("16", "Size=16")];
    let tree = with_hicolor(
        Tree::default()
            .file(&format!("{SHARE}/t/index.theme"), &index(None, &dir))
            .icon("/usr/share/icons/t/16/document-send.svg")
            .icon("/usr/share/icons/hicolor/48x48/apps/document-send-mail.png"),
    );
    let theme = load(&tree, "t").unwrap();
    assert_eq!(
        find(&theme, &tree, IconRequest::new("document-send-mail", 16)).as_deref(),
        Some("/usr/share/icons/t/16/document-send.svg")
    );
    assert_eq!(
        find(
            &theme,
            &tree,
            IconRequest::new("document-send-mail", 16).generic_fallback(false)
        )
        .as_deref(),
        Some("/usr/share/icons/hicolor/48x48/apps/document-send-mail.png")
    );
    let names = |request: IconRequest<'_>| -> Vec<String> {
        request.candidates().into_iter().map(|(n, _)| n).collect()
    };
    assert_eq!(
        names(IconRequest::new("go-down-symbolic", 16)),
        ["go-down-symbolic", "go-symbolic", "go-down", "go"]
    );
    assert_eq!(names(IconRequest::new("-symbolic", 16)), ["-symbolic"]);
}

// ---- Bounds ----------------------------------------------------------------

#[test]
fn a_name_that_would_leave_the_theme_is_refused() {
    let tree = with_hicolor(
        Tree::default()
            .file(
                &format!("{SHARE}/t/index.theme"),
                &index(Some("../../../etc,ok/../t"), &[("16", "Size=16")]),
            )
            .icon("/usr/share/icons/t/secret.svg")
            .icon("/usr/share/icons/t/16/x.svg")
            .icon("/usr/share/pixmaps/x.svg"),
    );
    let theme = load(&tree, "t").unwrap();
    assert_eq!(theme.chain().collect::<Vec<_>>(), ["t", "hicolor"]);
    for name in [
        "../t/secret",
        "16/x",
        "..",
        "",
        "x\0",
        "a\\b",
        "/usr/share/pixmaps/x",
    ] {
        assert_eq!(
            theme.lookup(&tree, &IconRequest::new(name, 16)),
            None,
            "{name:?}"
        );
    }
    assert!(load(&tree, "../icons/t").is_none());
    assert!(load(&tree, "t/").is_none());
}

#[test]
fn a_directory_that_would_leave_the_theme_is_ignored() {
    let text = "[Icon Theme]\nName=t\nDirectories=../../../../etc,/etc,16/../../x,./16,16\n\n\
                [../../../../etc]\nSize=16\n[/etc]\nSize=16\n[16/../../x]\nSize=16\n\
                [./16]\nSize=16\n[16]\nSize=16\n";
    let tree = with_hicolor(Tree::default().file("/usr/share/icons/t/index.theme", text));
    let theme = load(&tree, "t").unwrap();
    let dirs: Vec<&str> = theme.chain[0]
        .directories
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert_eq!(dirs, ["16"]);
}

#[test]
fn an_index_too_long_makes_its_theme_unusable() {
    let mut text = index(None, &[("16", "Size=16")]);
    text.push_str(&"#".repeat(MAX_INDEX_LEN as usize));
    let tree = with_hicolor(Tree::default().file("/usr/share/icons/t/index.theme", &text));
    assert!(load(&tree, "t").is_none());
}

#[test]
fn directories_past_the_bound_are_ignored() {
    let names: Vec<String> = (0..=MAX_DIRECTORIES).map(|n| format!("d{n}")).collect();
    let mut text = format!("[Icon Theme]\nName=t\nDirectories={}\n", names.join(","));
    for name in &names {
        text.push_str(&format!("[{name}]\nSize=16\n"));
    }
    let last = names.last().unwrap();
    let tree = with_hicolor(
        Tree::default()
            .file("/usr/share/icons/t/index.theme", &text)
            .icon(&format!("/usr/share/icons/t/{last}/x.svg")),
    );
    let theme = load(&tree, "t").unwrap();
    assert_eq!(theme.chain[0].directories.len(), MAX_DIRECTORIES);
    assert_eq!(theme.lookup(&tree, &IconRequest::new("x", 16)), None);
}

#[test]
fn the_real_reader_stops_after_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.theme");
    std::fs::write(&path, b"0123456789").unwrap();
    let reader = FileSystemIconReader;
    assert_eq!(reader.read(&path, 4).unwrap(), b"01234");
    assert_eq!(reader.read(&path, 100).unwrap(), b"0123456789");
    assert!(reader.is_file(&path));
    assert!(reader.is_dir(dir.path()));
    assert!(reader.read(&dir.path().join("missing"), 4).is_none());
}
