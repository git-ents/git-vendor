//! Tests for `track_vendor`.
//!
//! `track_vendor` appends `<path> vendor=<name>` lines to the working-copy
//! `.gitattributes` for each path not already attributed to this vendor.

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorWorktree as _};
use gix::bstr::ByteSlice as _;

use crate::support::{git, init, write};

fn entry() -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "up/**".to_owned(),
            destination: Some("vendor/".to_owned()),
        }],
        mode: VendorMode::Merge,
    }
}

struct Built {
    _dir: tempfile::TempDir,
    repo: gix::Repository,
}

/// A non-bare repo with an initial commit and a pre-existing `.gitattributes`.
fn build_with_attributes(attrs: &[u8]) -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"hello\n");
    write(dir.path(), ".gitattributes", attrs);
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    Built { _dir: dir, repo }
}

/// A non-bare repo with an initial commit and no `.gitattributes`.
fn build_without_attributes() -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"hello\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    Built { _dir: dir, repo }
}

/// Tracking new paths appends `<path> vendor=<name>` lines to `.gitattributes`.
#[test]
fn adds_attribute_lines_for_new_paths() {
    let b = build_without_attributes();
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .track_vendor(
            &entry(),
            &[b"vendor/a.txt".as_bstr(), b"vendor/b.txt".as_bstr()],
        )
        .expect("track_vendor");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    assert!(
        content.contains("vendor/a.txt vendor=mylib\n"),
        "missing a.txt line: {content:?}",
    );
    assert!(
        content.contains("vendor/b.txt vendor=mylib\n"),
        "missing b.txt line: {content:?}",
    );
}

/// Calling `track_vendor` twice for the same paths does not duplicate lines.
#[test]
fn idempotent_does_not_duplicate_lines() {
    let b = build_without_attributes();
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .track_vendor(&entry(), &[b"vendor/a.txt".as_bstr()])
        .expect("first call");
    b.repo
        .track_vendor(&entry(), &[b"vendor/a.txt".as_bstr()])
        .expect("second call");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    let count = content
        .lines()
        .filter(|l| *l == "vendor/a.txt vendor=mylib")
        .count();
    assert_eq!(count, 1, "line must appear exactly once: {content:?}");
}

/// When `.gitattributes` does not exist it is created from scratch.
#[test]
fn creates_gitattributes_when_absent() {
    let b = build_without_attributes();
    let workdir = b.repo.workdir().unwrap().to_owned();
    let attrs_path = workdir.join(".gitattributes");
    assert!(!attrs_path.exists(), "precondition: no .gitattributes");

    b.repo
        .track_vendor(&entry(), &[b"vendor/new.txt".as_bstr()])
        .expect("track_vendor");

    assert!(attrs_path.exists(), ".gitattributes must be created");
    let content = std::fs::read_to_string(&attrs_path).unwrap();
    assert_eq!(content, "vendor/new.txt vendor=mylib\n");
}

/// Pre-existing lines unrelated to this vendor are preserved.
#[test]
fn preserves_unrelated_lines() {
    let b = build_with_attributes(b"*.rs diff=rust\n");
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .track_vendor(&entry(), &[b"vendor/a.txt".as_bstr()])
        .expect("track_vendor");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    assert!(
        content.contains("*.rs diff=rust\n"),
        "pre-existing line must be preserved: {content:?}",
    );
    assert!(
        content.contains("vendor/a.txt vendor=mylib\n"),
        "new line must be appended: {content:?}",
    );
}

/// A path containing a space is rejected with `Error::InvalidPath`.
#[test]
fn path_with_space_returns_invalid_path_error() {
    let b = build_without_attributes();
    let err = b
        .repo
        .track_vendor(&entry(), &[b"vendor/a b.txt".as_bstr()])
        .unwrap_err();
    assert!(
        matches!(err, git_vendor::Error::InvalidPath(_)),
        "expected InvalidPath, got {err:?}",
    );
}

/// A bare repo returns `Error::NoWorkdir`.
#[test]
fn bare_repo_returns_no_workdir_error() {
    let dir = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], dir.path());
    let repo = gix::open(dir.path()).unwrap();
    let err = repo
        .track_vendor(&entry(), &[b"vendor/a.txt".as_bstr()])
        .unwrap_err();
    assert!(matches!(err, git_vendor::Error::NoWorkdir), "{err:?}");
}
