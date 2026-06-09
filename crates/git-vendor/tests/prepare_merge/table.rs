//! Tests for `prepare_merge`.
//!
//! `prepare_merge` writes the git-dir files that put the working copy into
//! merge state so the user can run `git commit` to seal the vendor merge.
//! In squash mode it writes `SQUASH_MSG`; in merge mode it writes `MERGE_HEAD`
//! and `MERGE_MSG`.

use git_vendor::{
    PatternMapping, VendorEntry, VendorMerge, VendorMode, VendorName, VendorWorktree as _,
};

use crate::support::{git, init, write};

fn null_oid() -> gix::ObjectId {
    gix::ObjectId::from_hex(b"0000000000000000000000000000000000000000").unwrap()
}

fn make_merge() -> VendorMerge {
    VendorMerge {
        upstream_commit: null_oid(),
        ancestor_tree: None,
        result_tree: null_oid(),
        conflicts: vec![],
    }
}

fn entry(mode: VendorMode) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "up/**".to_owned(),
            destination: Some("vendor/".to_owned()),
        }],
        mode,
    }
}

struct Built {
    _dir: tempfile::TempDir,
    repo: gix::Repository,
}

fn build_repo() -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"hello\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    Built { _dir: dir, repo }
}

/// Merge mode writes `MERGE_HEAD` containing the upstream commit OID followed
/// by a newline, and `MERGE_MSG` containing the message.
#[test]
fn merge_mode_writes_merge_head_and_merge_msg() {
    let b = build_repo();
    let merge = make_merge();
    let message = "vendor: update mylib\n";

    b.repo
        .prepare_merge(&entry(VendorMode::Merge), &merge, message)
        .expect("prepare_merge");

    let git_dir = b.repo.git_dir();

    let merge_head = std::fs::read_to_string(git_dir.join("MERGE_HEAD")).unwrap();
    assert_eq!(
        merge_head,
        format!("{}\n", null_oid()),
        "MERGE_HEAD must be <oid>\\n",
    );

    let merge_msg = std::fs::read_to_string(git_dir.join("MERGE_MSG")).unwrap();
    assert_eq!(
        merge_msg, message,
        "MERGE_MSG must equal the supplied message"
    );

    assert!(
        !git_dir.join("SQUASH_MSG").exists(),
        "SQUASH_MSG must not be written in merge mode",
    );
}

/// Squash mode writes `SQUASH_MSG` containing the message and does not write
/// `MERGE_HEAD` or `MERGE_MSG`.
#[test]
fn squash_mode_writes_squash_msg() {
    let b = build_repo();
    let merge = make_merge();
    let message = "vendor: add mylib (squash)\n";

    b.repo
        .prepare_merge(&entry(VendorMode::Squash), &merge, message)
        .expect("prepare_merge");

    let git_dir = b.repo.git_dir();

    let squash_msg = std::fs::read_to_string(git_dir.join("SQUASH_MSG")).unwrap();
    assert_eq!(
        squash_msg, message,
        "SQUASH_MSG must equal the supplied message",
    );

    assert!(
        !git_dir.join("MERGE_HEAD").exists(),
        "MERGE_HEAD must not be written in squash mode",
    );
    assert!(
        !git_dir.join("MERGE_MSG").exists(),
        "MERGE_MSG must not be written in squash mode",
    );
}

/// Calling `prepare_merge` a second time overwrites the previous state files.
#[test]
fn second_call_overwrites_previous_files() {
    let b = build_repo();
    let merge = make_merge();

    b.repo
        .prepare_merge(&entry(VendorMode::Merge), &merge, "first\n")
        .expect("first call");
    b.repo
        .prepare_merge(&entry(VendorMode::Merge), &merge, "second\n")
        .expect("second call");

    let merge_msg = std::fs::read_to_string(b.repo.git_dir().join("MERGE_MSG")).unwrap();
    assert_eq!(merge_msg, "second\n");
}
