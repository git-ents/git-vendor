//! Tests for `checkout_vendor`.
//!
//! `checkout_vendor` projects a vendor-only `tree` (local path space) onto the
//! one working copy and index: it writes every path the tree carries, removes
//! the vendor paths recorded in `HEAD` that the new tree no longer carries, and
//! leaves unrelated files untouched. Membership of the *old* set is read from
//! `HEAD`'s `.gitattributes` (`vendor=<name>`), so the fixture is a non-bare
//! repo with a committed working tree.

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorWorktree as _};
use gix::bstr::ByteSlice as _;

use crate::support::{build_tree, git, init, write};

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

/// The index paths after a checkout, in their on-disk order.
fn index_paths(repo: &gix::Repository) -> Vec<String> {
    let index = repo.open_index().unwrap();
    index
        .entries()
        .iter()
        .map(|e| e.path(&index).to_str_lossy().into_owned())
        .collect()
}

struct Built {
    _dir: tempfile::TempDir,
    repo: gix::Repository,
}

/// A non-bare repo whose `HEAD` carries `vendor/* vendor=mylib`, two vendored
/// files (`vendor/keep.txt`, `vendor/old.txt`), two unrelated files
/// (`README`, `.gitattributes`), and one untracked file (`untracked.txt`).
fn build() -> Built {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"local readme\n");
    write(dir.path(), ".gitattributes", b"vendor/* vendor=mylib\n");
    write(dir.path(), "vendor/keep.txt", b"v1\n");
    write(dir.path(), "vendor/old.txt", b"old\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "initial"], dir.path());
    write(dir.path(), "untracked.txt", b"untracked\n");
    let repo = gix::open(dir.path()).expect("gix open");
    Built { _dir: dir, repo }
}

/// The new tree's content is written verbatim to the working copy and recorded
/// in the index: an overwritten vendor file takes the new bytes, a brand-new
/// vendor file appears.
#[test]
fn writes_tree_to_working_copy_and_index() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    let tree = build_tree(
        &b.repo,
        &[("vendor/keep.txt", b"v2\n"), ("vendor/new.txt", b"new\n")],
    );

    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    assert_eq!(
        std::fs::read(workdir.join("vendor/keep.txt")).unwrap(),
        b"v2\n"
    );
    assert_eq!(
        std::fs::read(workdir.join("vendor/new.txt")).unwrap(),
        b"new\n"
    );

    let paths = index_paths(&b.repo);
    assert!(paths.contains(&"vendor/keep.txt".to_owned()), "{paths:?}");
    assert!(paths.contains(&"vendor/new.txt".to_owned()), "{paths:?}");
}

/// Vendor paths recorded in `HEAD` that the new tree no longer carries are
/// removed from both the working copy and the index.
#[test]
fn removes_vendor_paths_absent_from_new_tree() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);

    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    assert!(
        !workdir.join("vendor/old.txt").exists(),
        "stale vendor file must be removed from the working copy",
    );
    let paths = index_paths(&b.repo);
    assert!(
        !paths.contains(&"vendor/old.txt".to_owned()),
        "stale vendor path must be removed from the index: {paths:?}",
    );

    assert!(
        workdir.join("untracked.txt").exists(),
        "untracked content must not be modified"
    );
}

/// Files outside the vendor's set — tracked non-vendor files, the
/// `.gitattributes` that defines membership, and untracked
/// files — are left untouched in the working copy and the index.
#[test]
fn leaves_unrelated_files_untouched() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);

    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    assert_eq!(
        std::fs::read(workdir.join("README")).unwrap(),
        b"local readme\n"
    );
    assert_eq!(
        std::fs::read(workdir.join(".gitattributes")).unwrap(),
        b"vendor/* vendor=mylib\n",
    );

    let paths = index_paths(&b.repo);
    assert!(paths.contains(&"README".to_owned()), "{paths:?}");
    assert!(paths.contains(&".gitattributes".to_owned()), "{paths:?}");
    assert!(
        workdir.join("untracked.txt").exists(),
        "untracked content must not be modified"
    );
}

/// A bare repo has no workdir; `checkout_vendor` must surface `Error::NoWorkdir`
/// rather than panicking or silently succeeding.
#[test]
fn bare_repo_returns_no_workdir_error() {
    let dir = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], dir.path());
    let repo = gix::open(dir.path()).unwrap();
    let tree = gix::ObjectId::empty_tree(repo.object_hash());
    let err = repo.checkout_vendor(&entry(), tree).unwrap_err();
    assert!(matches!(err, git_vendor::Error::NoWorkdir), "{err:?}");
}

/// With no HEAD commit yet `head_commit()` returns `None`, so `old_paths` is
/// empty. The new tree should still be checked out cleanly.
#[test]
fn first_checkout_into_unborn_head_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    // No commits — HEAD is unborn
    let repo = gix::open(dir.path()).unwrap();
    let tree = build_tree(&repo, &[("vendor/new.txt", b"new\n")]);
    repo.checkout_vendor(&entry(), tree)
        .expect("first checkout into unborn HEAD");
    let workdir = repo.workdir().unwrap();
    assert_eq!(
        std::fs::read(workdir.join("vendor/new.txt")).unwrap(),
        b"new\n"
    );
}

/// A stale vendor path that is a dangling symlink in the working copy must be
/// removed: `symlink_metadata()` detects it regardless of whether the target
/// exists, and `remove_file` unlinks the symlink itself.
#[cfg(unix)]
#[test]
fn stale_dangling_symlink_is_removed() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    std::fs::remove_file(workdir.join("vendor/old.txt")).unwrap();
    std::os::unix::fs::symlink("nonexistent", workdir.join("vendor/old.txt")).unwrap();

    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);
    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    assert!(
        workdir.join("vendor/old.txt").symlink_metadata().is_err(),
        "stale dangling symlink must be removed",
    );
}

/// A stale vendor path that is a live symlink (target exists) is also removed.
#[cfg(unix)]
#[test]
fn stale_live_symlink_is_removed() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    // Replace vendor/old.txt with a symlink pointing to README (which exists).
    std::fs::remove_file(workdir.join("vendor/old.txt")).unwrap();
    std::os::unix::fs::symlink(workdir.join("README"), workdir.join("vendor/old.txt")).unwrap();

    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);
    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    assert!(
        workdir.join("vendor/old.txt").symlink_metadata().is_err(),
        "stale live symlink must be removed",
    );
    // The symlink target must be untouched.
    assert!(workdir.join("README").exists());
}

/// After all vendor files are removed the now-empty parent directory is
/// pruned, matching `remove`'s behavior (and git's own checkout, which does
/// not leave emptied directories behind).
#[test]
fn stale_empty_vendor_directory_is_pruned() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    // New tree carries no vendor files; both old vendor files are removed.
    let empty_tree = build_tree(&b.repo, &[]);
    b.repo
        .checkout_vendor(&entry(), empty_tree)
        .expect("checkout");

    assert!(!workdir.join("vendor/old.txt").exists());
    assert!(!workdir.join("vendor/keep.txt").exists());
    assert!(
        !workdir.join("vendor").exists(),
        "emptied vendor/ dir must be pruned after its last file is removed",
    );
}

/// The written index is in lexicographic order even when vendor paths sort
/// before some pre-existing non-vendor paths.
#[test]
fn index_entries_are_sorted_after_checkout() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    // z_file.txt sorts after a_vendor/*, so the vendor entry must not land
    // after z_file.txt in the index.
    write(dir.path(), "z_file.txt", b"z\n");
    write(dir.path(), ".gitattributes", b"a_vendor/* vendor=mylib\n");
    write(dir.path(), "a_vendor/old.txt", b"old\n");
    git(&["add", "-A"], dir.path());
    git(&["commit", "-m", "init"], dir.path());

    let repo = gix::open(dir.path()).unwrap();
    let custom_entry = VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "up/**".to_owned(),
            destination: Some("a_vendor/".to_owned()),
        }],
        mode: VendorMode::Merge,
    };

    let tree = build_tree(&repo, &[("a_vendor/new.txt", b"new\n")]);
    repo.checkout_vendor(&custom_entry, tree).expect("checkout");

    let index = repo.open_index().unwrap();
    let paths: Vec<gix::bstr::BString> = index
        .entries()
        .iter()
        .map(|e| e.path(&index).to_owned())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "index entries must be sorted after checkout");
}

/// A file staged but not yet committed must survive `checkout_vendor`.
/// Regression: the on-disk index was rebuilt from `full_tree`, which overlays
/// the vendor tree onto *HEAD's* committed tree — never the index — so any
/// staged-but-uncommitted addition was silently dropped from the index.
#[test]
fn staged_uncommitted_addition_is_preserved() {
    let b = build();
    let workdir = b.repo.workdir().unwrap().to_owned();
    write(&workdir, "newfile.txt", b"staged\n");
    git(&["add", "newfile.txt"], &workdir);

    let tree = build_tree(&b.repo, &[("vendor/keep.txt", b"v2\n")]);
    b.repo.checkout_vendor(&entry(), tree).expect("checkout");

    let paths = index_paths(&b.repo);
    assert!(
        paths.contains(&"newfile.txt".to_owned()),
        "staged addition must survive checkout: {paths:?}",
    );
}

/// The same regression on an unborn `HEAD`: staging a file before the first
/// commit, then running `checkout_vendor`, must not wipe out the entire
/// pre-existing index (there is no committed tree to fall back to at all in
/// this case, so the old behavior dropped every staged entry).
#[test]
fn staged_uncommitted_addition_survives_unborn_head_checkout() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    write(dir.path(), "README", b"local readme\n");
    git(&["add", "README"], dir.path());
    // No commit — HEAD is unborn.
    let repo = gix::open(dir.path()).unwrap();

    let tree = build_tree(&repo, &[("vendor/new.txt", b"new\n")]);
    repo.checkout_vendor(&entry(), tree)
        .expect("checkout into unborn HEAD");

    let paths = index_paths(&repo);
    assert!(
        paths.contains(&"README".to_owned()),
        "pre-existing staged entry must survive checkout: {paths:?}",
    );
}
