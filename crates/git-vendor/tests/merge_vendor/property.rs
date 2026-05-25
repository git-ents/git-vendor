//! Property-based tests for invariants of `merge_vendor`.
//!
//! `merge_vendor` is a pure object-database three-way merge. These properties
//! pin the invariants that must hold for *any* content:
//!
//! * **Determinism** — the same `(entry, ours, theirs)` always yields the same
//!   `result_tree`, `conflicts`, `upstream_commit`, and `ancestor_tree`.
//! * **Field contract** — `upstream_commit` is exactly `theirs`; with no
//!   recorded base, `ancestor_tree` is `None`.
//! * **Clean disjoint union** — when the local ("ours") and upstream
//!   ("theirs") filtered trees occupy disjoint local namespaces, a first add
//!   conflicts nowhere and the result is *exactly* the blob-for-blob union of
//!   the two filtered trees: nothing invented, dropped, or rewritten.
//! * **Identical add never conflicts** — a path added on both sides with
//!   byte-identical content is never a conflict, and carries that content.
//!
//! The local repo is bare and both histories are fetched in (fetching into a
//! non-bare repo on `main` resolves the wrong tip). Each case builds a fresh
//! scenario, so the case count is kept modest.

use std::collections::BTreeMap;
use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
use gix::bstr::ByteSlice as _;
use proptest::prelude::*;

fn git(args: &[&str], dir: &Path) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(std::process::Stdio::null())
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
}

fn git_capture(args: &[&str], dir: &Path) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
    output.stdout
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

fn entry(url: &str) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "up/**".to_owned(),
            destination: Some("vendor/".to_owned()),
        }],
        mode: VendorMode::Merge,
    }
}

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

/// Build a scratch worktree (`README` + `files` + optional `.gitattributes`)
/// and fetch its `HEAD` into the bare `local` repo under `dst_ref`.
fn commit_into(
    local: &Path,
    dst_ref: &str,
    files: &BTreeMap<String, Vec<u8>>,
    dir_prefix: &str,
    gitattributes: Option<&str>,
) -> gix::ObjectId {
    let scratch = tempfile::tempdir().unwrap();
    init(scratch.path());
    write(scratch.path(), "README", b"scratch");
    if let Some(ga) = gitattributes {
        write(scratch.path(), ".gitattributes", ga.as_bytes());
    }
    for (name, content) in files {
        write(scratch.path(), &format!("{dir_prefix}{name}"), content);
    }
    git(&["add", "-A"], scratch.path());
    git(&["commit", "-m", "scratch"], scratch.path());
    let hex = git_capture(&["rev-parse", "HEAD"], scratch.path());
    git(
        &[
            "fetch",
            scratch.path().to_str().unwrap(),
            &format!("+HEAD:{dst_ref}"),
        ],
        local,
    );
    gix::ObjectId::from_hex(hex.trim()).expect("commit oid")
}

struct Built {
    _bare: tempfile::TempDir,
    repo: gix::Repository,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
    entry: VendorEntry,
}

/// `local` files are attributed `vendor=mylib` via a `loc/* vendor=mylib`
/// rule; `upstream` files are written under `up/` and remapped to `vendor/` —
/// disjoint from `loc/` in local path space.
fn build(local: &BTreeMap<String, Vec<u8>>, upstream: &BTreeMap<String, Vec<u8>>) -> Built {
    let u = tempfile::tempdir().unwrap();
    init(u.path());
    write(u.path(), "README", b"u");
    for (name, content) in upstream {
        write(u.path(), &format!("up/{name}"), content);
    }
    git(&["add", "-A"], u.path());
    git(&["commit", "-m", "u"], u.path());

    let bare = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], bare.path());
    let repo = gix::open(bare.path()).expect("gix open");
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        local,
        "loc/",
        Some("loc/* vendor=mylib\n"),
    );
    let e = entry(u.path().to_str().unwrap());
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    Built {
        _bare: bare,
        repo,
        ours,
        theirs,
        entry: e,
    }
}

/// File names are lowercase ASCII so they are valid path components on any
/// filesystem; contents are arbitrary bytes (blobs need not be text). The
/// `BTreeMap` dedups names within a side.
fn file_set() -> impl Strategy<Value = BTreeMap<String, Vec<u8>>> {
    prop::collection::btree_map(
        "[a-z]{1,5}".prop_map(|n| format!("{n}.dat")),
        prop::collection::vec(any::<u8>(), 0..6),
        0..4,
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, ..ProptestConfig::default() })]

    /// Determinism, the field contract, and the clean-disjoint-union law, all
    /// over one freshly built scenario per case.
    #[test]
    fn disjoint_add_is_deterministic_and_exact_union(
        local in file_set(),
        upstream in file_set(),
    ) {
        let b = build(&local, &upstream);

        let m1 = b.repo.merge_vendor(&b.entry, b.ours, b.theirs).expect("merge_vendor");
        let m2 = b.repo.merge_vendor(&b.entry, b.ours, b.theirs).expect("merge_vendor");

        // Determinism across the whole outcome.
        prop_assert_eq!(m1.result_tree, m2.result_tree);
        prop_assert_eq!(&m1.conflicts, &m2.conflicts);
        prop_assert_eq!(m1.upstream_commit, m2.upstream_commit);
        prop_assert_eq!(m1.ancestor_tree, m2.ancestor_tree);

        // Field contract.
        prop_assert_eq!(m1.upstream_commit, b.theirs);
        prop_assert_eq!(m1.ancestor_tree, None);

        // Disjoint namespaces (`loc/` vs `vendor/`) ⟹ no conflict, and the
        // result is the exact blob-for-blob union of the two filtered trees.
        prop_assert!(m1.conflicts.is_empty(), "unexpected conflicts: {:?}", m1.conflicts);
        let ours_tree = b.repo.ours_tree(&b.entry, b.ours).expect("ours_tree");
        let their_tree = b.repo.upstream_tree(&b.entry, b.theirs).expect("upstream_tree");
        let mut union = tree_entries(&b.repo, ours_tree);
        union.extend(tree_entries(&b.repo, their_tree));
        prop_assert_eq!(tree_entries(&b.repo, m1.result_tree), union);
    }

    /// A path added on *both* sides with byte-identical content is never a
    /// conflict, and the result carries that exact content. The upstream file
    /// `up/<n>` maps to `vendor/<n>`; the local side attributes the same
    /// `vendor/<n>` path with the same bytes.
    #[test]
    fn identical_add_never_conflicts(
        name in "[a-z]{1,5}",
        content in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let file = format!("{name}.dat");

        let u = tempfile::tempdir().unwrap();
        init(u.path());
        write(u.path(), "README", b"u");
        write(u.path(), &format!("up/{file}"), &content);
        git(&["add", "-A"], u.path());
        git(&["commit", "-m", "u"], u.path());

        let bare = tempfile::tempdir().unwrap();
        git(&["init", "--bare", "-b", "main"], bare.path());
        let repo = gix::open(bare.path()).expect("gix open");
        let mut local = BTreeMap::new();
        local.insert(file.clone(), content.clone());
        let ours = commit_into(
            bare.path(),
            "refs/scratch/ours",
            &local,
            "vendor/",
            Some("vendor/* vendor=mylib\n"),
        );
        let e = entry(u.path().to_str().unwrap());
        let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

        let m = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");

        let local_path = format!("vendor/{file}");
        prop_assert!(
            !m.conflicts.iter().any(|c| c == &local_path),
            "identical add reported as conflict: {:?}",
            m.conflicts,
        );
        let entries = tree_entries(&repo, m.result_tree);
        let oid = entries.get(&local_path).expect("path present");
        let obj = repo.find_object(*oid).unwrap();
        prop_assert_eq!(obj.data.as_bstr(), content.as_bstr());
    }
}
