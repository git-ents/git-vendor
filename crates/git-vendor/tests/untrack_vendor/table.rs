//! Tests for `untrack_vendor`.
//!
//! `untrack_vendor` removes `<path> vendor=<name>` lines from the working-copy
//! `.gitattributes` for each path that matches.

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorWorktree as _};

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

/// Untracking removes the matching `<path> vendor=<name>` line.
#[test]
fn removes_attribute_line_for_given_path() {
    let b = build_with_attributes(b"vendor/a.txt vendor=mylib\nvendor/b.txt vendor=mylib\n");
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .untrack_vendor(&entry(), &["vendor/a.txt"])
        .expect("untrack_vendor");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    assert!(
        !content.contains("vendor/a.txt vendor=mylib"),
        "removed line must be absent: {content:?}",
    );
    assert!(
        content.contains("vendor/b.txt vendor=mylib\n"),
        "other vendor line must remain: {content:?}",
    );
}

/// Unrelated lines are not touched.
#[test]
fn preserves_unrelated_lines() {
    let b = build_with_attributes(b"*.rs diff=rust\nvendor/a.txt vendor=mylib\n");
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .untrack_vendor(&entry(), &["vendor/a.txt"])
        .expect("untrack_vendor");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    assert!(
        content.contains("*.rs diff=rust\n"),
        "unrelated line must be preserved: {content:?}",
    );
    assert!(
        !content.contains("vendor/a.txt vendor=mylib"),
        "removed line must be absent: {content:?}",
    );
}

/// Calling `untrack_vendor` for a path not in `.gitattributes` is a no-op.
#[test]
fn idempotent_path_not_present_is_noop() {
    let b = build_with_attributes(b"*.rs diff=rust\n");
    let workdir = b.repo.workdir().unwrap().to_owned();

    b.repo
        .untrack_vendor(&entry(), &["vendor/missing.txt"])
        .expect("untrack_vendor");

    let content = std::fs::read_to_string(workdir.join(".gitattributes")).unwrap();
    assert_eq!(content, "*.rs diff=rust\n");
}

/// When `.gitattributes` does not exist, the call succeeds without creating it.
#[test]
fn no_gitattributes_is_noop() {
    let b = build_without_attributes();
    let workdir = b.repo.workdir().unwrap().to_owned();
    let attrs_path = workdir.join(".gitattributes");
    assert!(!attrs_path.exists(), "precondition: no .gitattributes");

    b.repo
        .untrack_vendor(&entry(), &["vendor/a.txt"])
        .expect("untrack_vendor");

    assert!(!attrs_path.exists(), ".gitattributes must not be created");
}

/// A bare repo returns `Error::NoWorkdir`.
#[test]
fn bare_repo_returns_no_workdir_error() {
    let dir = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], dir.path());
    let repo = gix::open(dir.path()).unwrap();
    let err = repo
        .untrack_vendor(&entry(), &["vendor/a.txt"])
        .unwrap_err();
    assert!(matches!(err, git_vendor::Error::NoWorkdir), "{err:?}");
}
