//! Tests for `vendor_overlay`.
//!
//! `vendor_overlay` splices a vendor-only `vendor_tree` (local path space) into
//! the full tree of `base_commit`: it removes the paths `base_commit` records as
//! owned by the vendor (via `.gitattributes` `vendor=<name>`) — which may have
//! been renamed or dropped — then upserts every entry from `vendor_tree`. The
//! result carries the vendor content alongside every non-vendored file. It is a
//! pure object-database function: no ref or working tree is touched.
//!
//! The fixture is a non-bare repo whose `HEAD` carries the membership
//! `.gitattributes` and a mix of vendored and unrelated files; only the commit
//! object database is read.

use std::collections::BTreeMap;
use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
use gix::bstr::ByteSlice as _;

// ── git helpers ───────────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(std::process::Stdio::null())
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn write(dir: &Path, rel: &str, contents: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn init(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
}

fn entry(name: &str) -> VendorEntry {
    VendorEntry {
        name: VendorName::new(name).unwrap(),
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

/// Build a tree in `repo`'s object database from `path -> bytes` pairs.
fn build_tree(repo: &gix::Repository, files: &[(&str, &[u8])]) -> gix::ObjectId {
    let mut editor = repo.empty_tree().edit().unwrap();
    for (path, bytes) in files {
        let blob = repo.write_blob(bytes).unwrap().detach();
        editor
            .upsert(*path, gix::objs::tree::EntryKind::Blob, blob)
            .unwrap();
    }
    editor.write().unwrap().detach()
}

/// Flatten a tree into `path -> blob oid`, skipping trees.
fn tree_entries(repo: &gix::Repository, tree: gix::ObjectId) -> BTreeMap<String, gix::ObjectId> {
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| (r.filepath.to_str_lossy().into_owned(), r.oid))
        .collect()
}

fn blob_at(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Vec<u8> {
    let oid = tree_entries(repo, tree)
        .get(path)
        .copied()
        .unwrap_or_else(|| panic!("path {path:?} absent from tree"));
    repo.find_object(oid).expect("find blob").data.clone()
}

struct Built {
    _dir: tempfile::TempDir,
    repo: gix::Repository,
    base: gix::ObjectId,
}

/// A non-bare repo whose `HEAD` carries `vendor/* vendor=mylib`, two vendored
/// files (`vendor/keep.txt`, `vendor/old.txt`) and two unrelated files
/// (`README`, `.gitattributes`).
fn build() -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"local readme\n");
    write(dir.path(), ".gitattributes", b"vendor/* vendor=mylib\n");
    write(dir.path(), "vendor/keep.txt", b"v1\n");
    write(dir.path(), "vendor/old.txt", b"old\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());

    let repo = gix::open(dir.path()).expect("gix open");
    let base = repo.head_commit().expect("head_commit").id().detach();
    Built {
        _dir: dir,
        repo,
        base,
    }
}

/// The vendor tree is spliced onto the base tree: an overwritten vendor file
/// takes the new bytes, a brand-new vendor file appears, and the non-vendored
/// files survive with their original bytes.
#[test]
fn splices_vendor_tree_onto_base_tree() {
    let b = build();
    let tree = build_tree(
        &b.repo,
        &[("vendor/keep.txt", b"v2\n"), ("vendor/new.txt", b"new\n")],
    );

    let result = b
        .repo
        .vendor_overlay(&entry("mylib"), b.base, tree)
        .expect("vendor_overlay");

    let paths: Vec<String> = tree_entries(&b.repo, result).into_keys().collect();
    assert_eq!(
        paths,
        vec![
            ".gitattributes".to_owned(),
            "README".to_owned(),
            "vendor/keep.txt".to_owned(),
            "vendor/new.txt".to_owned(),
        ],
    );
    assert_eq!(blob_at(&b.repo, result, "README"), b"local readme\n");
    assert_eq!(
        blob_at(&b.repo, result, ".gitattributes"),
        b"vendor/* vendor=mylib\n",
    );
    assert_eq!(blob_at(&b.repo, result, "vendor/keep.txt"), b"v2\n");
    assert_eq!(blob_at(&b.repo, result, "vendor/new.txt"), b"new\n");
}

/// Vendor paths the base records as owned (`vendor/old.txt`) that the new tree
/// no longer carries are removed from the result.
#[test]
fn removes_owned_paths_absent_from_vendor_tree() {
    let b = build();
    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);

    let result = b
        .repo
        .vendor_overlay(&entry("mylib"), b.base, tree)
        .expect("vendor_overlay");

    let paths: Vec<String> = tree_entries(&b.repo, result).into_keys().collect();
    assert!(
        !paths.contains(&"vendor/old.txt".to_owned()),
        "dropped vendor path must be removed: {paths:?}",
    );
}

/// A vendor whose name owns no path in the base (here `other`, while the base
/// marks files `vendor=mylib`) removes nothing: the full base tree is preserved
/// and the vendor tree is added on top.
#[test]
fn removes_nothing_when_vendor_owns_no_base_path() {
    let b = build();
    let tree = build_tree(&b.repo, &[("third_party/lib.txt", b"x\n")]);

    let result = b
        .repo
        .vendor_overlay(&entry("other"), b.base, tree)
        .expect("vendor_overlay");

    let paths: Vec<String> = tree_entries(&b.repo, result).into_keys().collect();
    assert_eq!(
        paths,
        vec![
            ".gitattributes".to_owned(),
            "README".to_owned(),
            "third_party/lib.txt".to_owned(),
            "vendor/keep.txt".to_owned(),
            "vendor/old.txt".to_owned(),
        ],
        "base files all survive, with the vendor tree added on top",
    );
}

/// The non-vendored files are carried over by identity — same blob oids as in
/// the base tree, not merely the same bytes.
#[test]
fn non_vendored_files_keep_their_object_identity() {
    let b = build();
    let base_tree = b
        .repo
        .find_commit(b.base)
        .unwrap()
        .tree_id()
        .unwrap()
        .detach();
    let base_entries = tree_entries(&b.repo, base_tree);

    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);
    let result = b
        .repo
        .vendor_overlay(&entry("mylib"), b.base, tree)
        .expect("vendor_overlay");
    let result_entries = tree_entries(&b.repo, result);

    assert_eq!(result_entries["README"], base_entries["README"]);
    assert_eq!(
        result_entries[".gitattributes"],
        base_entries[".gitattributes"],
    );
}

/// `vendor_overlay` is a pure function of its arguments: identical inputs yield
/// the identical result tree oid.
#[test]
fn is_deterministic() {
    let b = build();
    let tree = build_tree(
        &b.repo,
        &[("vendor/keep.txt", b"v2\n"), ("vendor/new.txt", b"new\n")],
    );

    let once = b
        .repo
        .vendor_overlay(&entry("mylib"), b.base, tree)
        .expect("vendor_overlay");
    let twice = b
        .repo
        .vendor_overlay(&entry("mylib"), b.base, tree)
        .expect("vendor_overlay");
    assert_eq!(once, twice);
}

/// A `base_commit` that is not a commit (here a tree oid) is an error, not a
/// panic.
#[test]
fn non_commit_base_is_error() {
    let b = build();
    let base_tree = b
        .repo
        .find_commit(b.base)
        .unwrap()
        .tree_id()
        .unwrap()
        .detach();
    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);

    assert!(
        b.repo
            .vendor_overlay(&entry("mylib"), base_tree, tree)
            .is_err(),
    );
}

/// A well-formed but absent `base_commit` oid is an error, not a panic.
#[test]
fn missing_base_is_error() {
    let b = build();
    let bogus =
        gix::ObjectId::from_hex(b"ffffffffffffffffffffffffffffffffffffffff").expect("hex oid");
    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);

    assert!(b.repo.vendor_overlay(&entry("mylib"), bogus, tree).is_err());
}
