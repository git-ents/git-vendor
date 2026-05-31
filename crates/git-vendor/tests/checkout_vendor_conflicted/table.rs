//! Tests for `checkout_vendor_conflicted`.
//!
//! `checkout_vendor_conflicted` writes the merge result tree (with conflict
//! markers) to the working copy and splices the unmerged stage 1/2/3 entries
//! into the index, reproducing the state that a stalled `git merge` leaves:
//! `git ls-files -u` lists the conflicting paths and `git add` resolves them.

use git_vendor::{
    ConflictStages, PatternMapping, VendorEntry, VendorMerge, VendorMode, VendorName,
    VendorWorktree as _,
};
use gix::bstr::ByteSlice as _;
use gix::objs::tree::EntryKind;

use crate::support::{build_tree, git, git_capture, init, write};

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

/// A non-bare repo with a committed `.gitattributes` assigning `vendor=mylib`,
/// a vendored file (`vendor/file.txt`), and an unrelated file (`README`).
fn build() -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"local readme\n");
    write(dir.path(), ".gitattributes", b"vendor/* vendor=mylib\n");
    write(dir.path(), "vendor/file.txt", b"base content\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    Built { _dir: dir, repo }
}

/// Write `contents` as a blob into `repo`'s object database, return the OID.
fn write_blob(repo: &gix::Repository, contents: &[u8]) -> gix::ObjectId {
    repo.write_blob(contents).unwrap().detach()
}

/// A textual conflict with markers in the result tree causes the working-copy
/// file to contain `<<<<<<<` markers, and `git ls-files -u` shows the three
/// unmerged stages.
#[test]
fn conflict_markers_in_working_copy_and_unmerged_in_index() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();

    let base_content = b"base content\n";
    let ours_content = b"ours content\n";
    let theirs_content = b"theirs content\n";
    // Simulate a textual conflict: the result blob carries conflict markers.
    let result_content = b"<<<<<<< ours\nours content\n=======\ntheirs content\n>>>>>>> theirs\n";

    let base_blob = write_blob(&b.repo, base_content);
    let ours_blob = write_blob(&b.repo, ours_content);
    let theirs_blob = write_blob(&b.repo, theirs_content);
    let result_blob = write_blob(&b.repo, result_content);

    let mode = gix::objs::tree::EntryMode::from(EntryKind::Blob);

    let result_tree = {
        let mut editor = b.repo.empty_tree().edit().unwrap();
        editor
            .upsert("vendor/file.txt", EntryKind::Blob, result_blob)
            .unwrap();
        editor.write().unwrap().detach()
    };

    let merge = VendorMerge {
        upstream_commit: gix::ObjectId::null(b.repo.object_hash()),
        ancestor_tree: None,
        result_tree,
        conflicts: vec![ConflictStages {
            path: "vendor/file.txt".to_owned(),
            stages: [
                Some((mode, base_blob)),
                Some((mode, ours_blob)),
                Some((mode, theirs_blob)),
            ],
        }],
    };

    b.repo
        .checkout_vendor_conflicted(&entry(), &merge)
        .expect("checkout_vendor_conflicted");

    // 1. The working-copy file contains conflict markers.
    let on_disk =
        std::fs::read(workdir.join("vendor/file.txt")).expect("vendor/file.txt must exist");
    assert!(
        on_disk.find(b"<<<<<<<").is_some(),
        "working-copy file must contain conflict markers"
    );

    // 2. `git ls-files -u` shows unmerged entries for vendor/file.txt.
    let ls = git_capture(
        &["ls-files", "-u", "--", "vendor/file.txt"],
        workdir.as_ref(),
    );
    let ls_str = String::from_utf8_lossy(&ls);
    assert!(
        ls_str.contains("vendor/file.txt"),
        "ls-files -u must show unmerged entry: {ls_str:?}"
    );
    // All three stages present.
    assert_eq!(
        ls_str.lines().count(),
        3,
        "all three stages must be recorded: {ls_str:?}"
    );
}

/// An add/add conflict (no base stage) records only stages 2 and 3 in the
/// index, and the result tree contains the marker-augmented content.
#[test]
fn add_add_conflict_has_no_base_stage() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();

    let ours_content = b"new file ours\n";
    let theirs_content = b"new file theirs\n";
    let result_content = b"<<<<<<< ours\nnew file ours\n=======\nnew file theirs\n>>>>>>> theirs\n";

    let ours_blob = write_blob(&b.repo, ours_content);
    let theirs_blob = write_blob(&b.repo, theirs_content);

    let mode = gix::objs::tree::EntryMode::from(EntryKind::Blob);

    let result_tree = build_tree(
        &b.repo,
        &[
            ("vendor/file.txt", b"base content\n"),
            ("vendor/new.txt", result_content),
        ],
    );

    let merge = VendorMerge {
        upstream_commit: gix::ObjectId::null(b.repo.object_hash()),
        ancestor_tree: None,
        result_tree,
        conflicts: vec![ConflictStages {
            path: "vendor/new.txt".to_owned(),
            // No stage 1 (base) for an add/add conflict.
            stages: [None, Some((mode, ours_blob)), Some((mode, theirs_blob))],
        }],
    };

    b.repo
        .checkout_vendor_conflicted(&entry(), &merge)
        .expect("checkout_vendor_conflicted");

    // Working-copy file has conflict markers.
    let on_disk = std::fs::read(workdir.join("vendor/new.txt")).expect("vendor/new.txt must exist");
    assert!(
        on_disk.find(b"<<<<<<<").is_some(),
        "working-copy file must contain conflict markers"
    );

    // `git ls-files -u` shows two stages (2 and 3) but not stage 1.
    let ls = git_capture(
        &["ls-files", "-u", "--", "vendor/new.txt"],
        workdir.as_ref(),
    );
    let ls_str = String::from_utf8_lossy(&ls);
    assert!(
        ls_str.contains("vendor/new.txt"),
        "ls-files -u must show unmerged entry: {ls_str:?}"
    );
    assert_eq!(
        ls_str.lines().count(),
        2,
        "only stages 2 and 3 must be recorded: {ls_str:?}"
    );

    // The non-conflicted vendor/file.txt is at stage 0 (no entry in -u output).
    let ls_file = git_capture(
        &["ls-files", "-u", "--", "vendor/file.txt"],
        workdir.as_ref(),
    );
    assert!(
        ls_file.is_empty(),
        "non-conflicted file must not appear in ls-files -u"
    );

    // Unrelated files are untouched.
    assert_eq!(
        std::fs::read(workdir.join("README")).unwrap(),
        b"local readme\n"
    );
}
