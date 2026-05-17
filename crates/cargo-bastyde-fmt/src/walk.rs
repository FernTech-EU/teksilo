//! Recursive `.rs` file discovery.
//!
//! Walks a directory tree depth-first, yielding every `*.rs` file.
//! Skips `target/` directories anywhere in the tree (build output is
//! never user code) and any path component starting with `.` other
//! than `.` itself (hidden dirs like `.git`, `.cargo`).
//!
//! Intentionally uses only `std::fs` — the workspace style favors
//! lean dependencies, and recursion depth is bounded by repository
//! shape in practice.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn collect(paths: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        if !p.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("path not found: {}", p.display()),
            ));
        }
        if p.is_file() {
            if is_rust_file(p) {
                out.push(p.clone());
            }
            continue;
        }
        // Explicit user-supplied directory: read it unconditionally
        // (the skip-hidden/skip-target rule applies only when we
        // descend into sub-dirs from there).
        read_dir_into(p, &mut out)?;
    }
    Ok(out)
}

fn read_dir_into(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            read_dir_into(&path, out)?;
        } else if ft.is_file() && is_rust_file(&path) {
            out.push(path);
        }
        // Symlinks: skip silently. A formatter shouldn't follow them.
    }
    Ok(())
}

fn is_rust_file(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()) == Some("rs")
}

fn should_skip_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if name == "target" {
        return true;
    }
    // Skip hidden dirs (`.git`, `.cargo`, …) but not `.` itself.
    name.starts_with('.') && name != "."
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn finds_rs_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "").unwrap();
        fs::write(tmp.path().join("b.txt"), "").unwrap();
        let found = collect(&[tmp.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.rs"));
    }

    #[test]
    fn skips_target_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("target")).unwrap();
        fs::write(tmp.path().join("target").join("a.rs"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        let found = collect(&[tmp.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("b.rs"));
    }

    #[test]
    fn skips_hidden_dirs() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".git").join("a.rs"), "").unwrap();
        fs::write(tmp.path().join("b.rs"), "").unwrap();
        let found = collect(&[tmp.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("b.rs"));
    }

    #[test]
    fn handles_explicit_file_path() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.rs");
        fs::write(&f, "").unwrap();
        let found = collect(std::slice::from_ref(&f)).unwrap();
        assert_eq!(found, vec![f]);
    }

    #[test]
    fn missing_path_errors() {
        let err = collect(&[PathBuf::from("/nonexistent/path/xyz")]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
