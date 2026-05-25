//! Table-driven and procedural tests for `merge_vendor`.
//!
//! `merge_vendor` is a pure object-database three-way merge. The "ours" side
//! is the attribute-filtered tree of a local commit
//! ([`ours_tree`](git_vendor::VendorRepository::ours_tree)); the "theirs" side
//! is the pattern-filtered, remapped tree of an *upstream* commit
//! ([`upstream_tree`](git_vendor::VendorRepository::upstream_tree)); the
//! ancestor is [`base_tree`](git_vendor::VendorRepository::base_tree) — the
//! empty tree before the first merge, which makes the operation an "add". All
//! three trees live in local path space, so conflict paths and the result
//! tree are local.
//!
//! The local repo is *bare* and the upstream / "ours" histories are built in
//! scratch worktrees and fetched in — the same fixture shape the `fetch`-based
//! tests use, since fetching into a non-bare repo that is itself on `main`
//! resolves the wrong tip.
//!
//! Cases that share the "first add" setup (no recorded base) are table-driven;
//! the three-way update cases and the resolution-content / error cases each
//! need a bespoke history and are procedural.

use std::collections::BTreeMap;
use std::path::Path;

use git_vendor::{
    Error, PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _,
};
use gix::bstr::ByteSlice as _;
use rstest::rstest;

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

fn git_capture(args: &[&str], dir: &Path) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
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

fn pat(glob: &str, destination: Option<&str>) -> PatternMapping {
    PatternMapping {
        glob: glob.to_owned(),
        destination: destination.map(str::to_owned),
    }
}

fn entry(url: &str, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: None,
        base: None,
        patterns,
        mode: VendorMode::Merge,
    }
}

/// A bare repo that vendor objects are fetched into.
fn make_bare() -> (tempfile::TempDir, gix::Repository) {
    let dir = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    (dir, repo)
}

/// Build a scratch worktree with one commit (`README` plus `files`, and an
/// optional `.gitattributes`), then fetch its `HEAD` into the bare `local`
/// repo under `dst_ref` so the objects live there. Returns the commit oid.
fn commit_into(
    local: &Path,
    dst_ref: &str,
    files: &[(&str, &[u8])],
    gitattributes: Option<&str>,
) -> gix::ObjectId {
    let scratch = tempfile::tempdir().unwrap();
    init(scratch.path());
    write(scratch.path(), "README", b"scratch");
    if let Some(ga) = gitattributes {
        write(scratch.path(), ".gitattributes", ga.as_bytes());
    }
    for (rel, contents) in files {
        write(scratch.path(), rel, contents);
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

fn paths(repo: &gix::Repository, tree: gix::ObjectId) -> Vec<String> {
    tree_entries(repo, tree).into_keys().collect()
}

fn blob(repo: &gix::Repository, oid: gix::ObjectId) -> Vec<u8> {
    repo.find_object(oid).expect("find blob").data.clone()
}

fn blob_at(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Vec<u8> {
    let oid = tree_entries(repo, tree)
        .get(path)
        .copied()
        .unwrap_or_else(|| panic!("path {path:?} absent from result tree"));
    blob(repo, oid)
}

struct Built {
    _bare: tempfile::TempDir,
    _upstream: tempfile::TempDir,
    repo: gix::Repository,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
    entry: VendorEntry,
}

/// First-"add" builder: a bare local repo with the local ("ours") commit
/// fetched in (carrying `gitattributes`) and an upstream commit fetched via
/// `fetch_vendor`. No recorded base.
fn build(
    local_files: &[(&str, &[u8])],
    gitattributes: &str,
    upstream_files: &[(&str, &[u8])],
    patterns: Vec<PatternMapping>,
) -> Built {
    let upstream = tempfile::tempdir().unwrap();
    init(upstream.path());
    write(upstream.path(), "README", b"upstream");
    for (rel, contents) in upstream_files {
        write(upstream.path(), rel, contents);
    }
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "upstream"], upstream.path());

    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        local_files,
        Some(gitattributes),
    );
    let e = entry(upstream.path().to_str().unwrap(), patterns);
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    Built {
        _bare: bare,
        _upstream: upstream,
        repo,
        ours,
        theirs,
        entry: e,
    }
}

// ── First "add": table-driven path / conflict-set behavior ────────────────────

/// Every first-add case shares the builder: a local commit with a
/// `vendor=mylib` attribute rule and an upstream commit remapped into the same
/// local path space. The table fixes the local content, the upstream content,
/// and asserts the exact result-tree path set and the exact (sorted,
/// deduplicated) conflict path list.
///
/// * disjoint namespaces  → clean union, no conflicts
/// * same path, identical → clean, single entry, no conflict
/// * same path, differing text   → add/add conflict at that path
/// * same path, differing binary → add/add conflict at that path
/// * two differing paths  → both reported, sorted
#[rstest]
#[case::disjoint_clean_union(
    vec![("loc/keep.txt", &b"local"[..])],
    "loc/* vendor=mylib\n",
    vec![("up/x.txt", &b"upstream"[..])],
    vec![pat("up/**", Some("vendor/"))],
    &["loc/keep.txt", "vendor/x.txt"],
    &[],
)]
#[case::identical_same_path_is_clean(
    vec![("vendor/f.txt", &b"same"[..])],
    "vendor/* vendor=mylib\n",
    vec![("up/f.txt", &b"same"[..])],
    vec![pat("up/**", Some("vendor/"))],
    &["vendor/f.txt"],
    &[],
)]
#[case::text_conflict_same_path(
    vec![("vendor/f.txt", &b"local\n"[..])],
    "vendor/* vendor=mylib\n",
    vec![("up/f.txt", &b"upstream\n"[..])],
    vec![pat("up/**", Some("vendor/"))],
    &["vendor/f.txt"],
    &["vendor/f.txt"],
)]
#[case::binary_conflict_same_path(
    vec![("vendor/b.bin", &b"\x00local"[..])],
    "vendor/* vendor=mylib\n",
    vec![("up/b.bin", &b"\x00upstream"[..])],
    vec![pat("up/**", Some("vendor/"))],
    &["vendor/b.bin"],
    &["vendor/b.bin"],
)]
#[case::multiple_conflicts_sorted_and_deduped(
    vec![("vendor/z.txt", &b"lz\n"[..]), ("vendor/a.txt", &b"la\n"[..])],
    "vendor/* vendor=mylib\n",
    vec![("up/z.txt", &b"uz\n"[..]), ("up/a.txt", &b"ua\n"[..])],
    vec![pat("up/**", Some("vendor/"))],
    &["vendor/a.txt", "vendor/z.txt"],
    &["vendor/a.txt", "vendor/z.txt"],
)]
fn first_add_paths_and_conflicts(
    #[case] local_files: Vec<(&str, &[u8])>,
    #[case] gitattributes: &str,
    #[case] upstream_files: Vec<(&str, &[u8])>,
    #[case] patterns: Vec<PatternMapping>,
    #[case] expected_paths: &[&str],
    #[case] expected_conflicts: &[&str],
) {
    let b = build(&local_files, gitattributes, &upstream_files, patterns);

    let merge = b
        .repo
        .merge_vendor(&b.entry, b.ours, b.theirs)
        .expect("merge_vendor");

    assert_eq!(paths(&b.repo, merge.result_tree), expected_paths);
    assert_eq!(merge.conflicts, expected_conflicts);
    // The contract: a first add has no recorded base, so the ancestor is
    // absent and the upstream commit is exactly `theirs`.
    assert_eq!(merge.ancestor_tree, None);
    assert_eq!(merge.upstream_commit, b.theirs);
    assert_eq!(merge.has_conflicts(), !expected_conflicts.is_empty());
}

// ── First "add": disjoint clean union is exactly ours ∪ theirs ────────────────

/// A clean first add over disjoint namespaces is the exact union of the two
/// filtered trees, blob-for-blob: no path is invented, dropped, or rewritten.
/// (The filtered trees are asserted non-empty so the union is a real check,
/// not vacuously satisfied by an empty upstream side.)
#[test]
fn disjoint_add_is_exact_union_of_filtered_trees() {
    let b = build(
        &[("loc/a.txt", b"A"), ("loc/b.txt", b"B")],
        "loc/* vendor=mylib\n",
        &[("up/c.txt", b"C"), ("up/d.txt", b"D")],
        vec![pat("up/**", Some("vendor/"))],
    );

    let merge = b
        .repo
        .merge_vendor(&b.entry, b.ours, b.theirs)
        .expect("merge_vendor");

    let ours_tree = b.repo.ours_tree(&b.entry, b.ours).expect("ours_tree");
    let their_tree = b
        .repo
        .upstream_tree(&b.entry, b.theirs)
        .expect("upstream_tree");
    let ours_e = tree_entries(&b.repo, ours_tree);
    let their_e = tree_entries(&b.repo, their_tree);
    assert_eq!(ours_e.len(), 2, "ours side must contribute");
    assert_eq!(their_e.len(), 2, "theirs side must contribute");

    let mut union = ours_e;
    union.extend(their_e);

    assert!(merge.conflicts.is_empty());
    assert_eq!(tree_entries(&b.repo, merge.result_tree), union);
}

// ── First "add": resolution content (markers vs. binary-keep-ours) ────────────

/// A textual add/add conflict is written with conflict markers carrying the
/// `ours`/`theirs` labels — the local ("ours") and upstream ("theirs") content
/// are both present in the blob, not silently dropped.
#[test]
fn text_conflict_writes_markers_with_labels() {
    let b = build(
        &[("vendor/f.txt", b"local-line\n")],
        "vendor/* vendor=mylib\n",
        &[("up/f.txt", b"upstream-line\n")],
        vec![pat("up/**", Some("vendor/"))],
    );

    let merge = b
        .repo
        .merge_vendor(&b.entry, b.ours, b.theirs)
        .expect("merge_vendor");

    assert_eq!(merge.conflicts, &["vendor/f.txt"]);
    let body = blob_at(&b.repo, merge.result_tree, "vendor/f.txt");
    let text = body.as_bstr().to_string();
    assert!(text.contains("<<<<<<< ours"), "no ours marker in {text:?}");
    assert!(
        text.contains(">>>>>>> theirs"),
        "no theirs marker in {text:?}"
    );
    assert!(
        text.contains("local-line"),
        "ours content missing: {text:?}"
    );
    assert!(
        text.contains("upstream-line"),
        "theirs content missing: {text:?}"
    );
}

/// A binary add/add conflict keeps the local ("ours") blob verbatim — no
/// markers are injected into binary content — yet the path is still reported
/// as a conflict so the caller can act on it.
#[test]
fn binary_conflict_keeps_ours_blob_verbatim() {
    let ours_bytes: &[u8] = b"\x00\x01local\xff";
    let b = build(
        &[("vendor/b.bin", ours_bytes)],
        "vendor/* vendor=mylib\n",
        &[("up/b.bin", b"\x00\x01upstream\xfe")],
        vec![pat("up/**", Some("vendor/"))],
    );

    let merge = b
        .repo
        .merge_vendor(&b.entry, b.ours, b.theirs)
        .expect("merge_vendor");

    assert_eq!(merge.conflicts, &["vendor/b.bin"]);
    assert_eq!(
        blob_at(&b.repo, merge.result_tree, "vendor/b.bin"),
        ours_bytes,
        "binary conflict must keep the local blob byte-for-byte",
    );
}

// ── Three-way "update": a recorded base is the merge ancestor ─────────────────

/// Build an upstream worktree with two commits: `files_v1` first (its commit
/// id returned, to be the recorded base), then `mutate` applied and committed.
fn upstream_two_commits(
    upstream: &Path,
    files_v1: &[(&str, &[u8])],
    mutate: impl FnOnce(&Path),
) -> gix::ObjectId {
    init(upstream);
    write(upstream, "README", b"upstream");
    for (rel, contents) in files_v1 {
        write(upstream, rel, contents);
    }
    git(&["add", "-A"], upstream);
    git(&["commit", "-m", "v1"], upstream);
    let c1 = git_capture(&["rev-parse", "HEAD"], upstream);
    let c1 = gix::ObjectId::from_hex(c1.trim()).expect("commit oid");

    mutate(upstream);
    git(&["add", "-A"], upstream);
    git(&["commit", "-m", "v2"], upstream);
    c1
}

fn update_entry(url: &str, base: gix::ObjectId) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: None,
        base: Some(base),
        patterns: vec![pat("src/**", Some("lib/"))],
        mode: VendorMode::Merge,
    }
}

/// Upstream changed the file, the local side did not: the upstream change is
/// taken and there is no conflict. The recorded base is the real merge
/// ancestor, surfaced as `ancestor_tree`.
#[test]
fn update_takes_upstream_only_change() {
    let upstream = tempfile::tempdir().unwrap();
    let base = upstream_two_commits(upstream.path(), &[("src/f.txt", b"v1\n")], |d| {
        write(d, "src/f.txt", b"v2\n");
    });
    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        &[("lib/f.txt", b"v1\n")],
        Some("lib/* vendor=mylib\n"),
    );
    let e = update_entry(upstream.path().to_str().unwrap(), base);
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    let merge = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");

    assert!(merge.conflicts.is_empty(), "{:?}", merge.conflicts);
    assert_eq!(blob_at(&repo, merge.result_tree, "lib/f.txt"), b"v2\n");
    assert_eq!(merge.ancestor_tree, repo.base_tree(&e).expect("base_tree"));
    assert_eq!(merge.upstream_commit, theirs);
}

/// The local side changed the file, upstream did not (base == theirs commit):
/// the local change is kept and there is no conflict.
#[test]
fn update_keeps_local_only_change() {
    let upstream = tempfile::tempdir().unwrap();
    init(upstream.path());
    write(upstream.path(), "README", b"upstream");
    write(upstream.path(), "src/f.txt", b"v1\n");
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "v1"], upstream.path());

    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        &[("lib/f.txt", b"local-edit\n")],
        Some("lib/* vendor=mylib\n"),
    );
    // Probe-fetch to learn the tip, then pin the base to that same commit so
    // there is no upstream change across the merge.
    let probe = entry(
        upstream.path().to_str().unwrap(),
        vec![pat("src/**", Some("lib/"))],
    );
    let tip = repo.fetch_vendor(&probe).expect("fetch_vendor");
    let e = update_entry(upstream.path().to_str().unwrap(), tip);

    let merge = repo.merge_vendor(&e, ours, tip).expect("merge_vendor");

    assert!(merge.conflicts.is_empty(), "{:?}", merge.conflicts);
    assert_eq!(
        blob_at(&repo, merge.result_tree, "lib/f.txt"),
        b"local-edit\n"
    );
}

/// Both sides changed the same path divergently from the base: the path is
/// reported as a conflict and the markered body is still written.
#[test]
fn update_both_sides_changed_conflicts() {
    let upstream = tempfile::tempdir().unwrap();
    let base = upstream_two_commits(upstream.path(), &[("src/f.txt", b"base\n")], |d| {
        write(d, "src/f.txt", b"upstream-change\n");
    });
    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        &[("lib/f.txt", b"local-change\n")],
        Some("lib/* vendor=mylib\n"),
    );
    let e = update_entry(upstream.path().to_str().unwrap(), base);
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    let merge = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");

    assert_eq!(merge.conflicts, &["lib/f.txt"]);
    let body = blob_at(&repo, merge.result_tree, "lib/f.txt")
        .as_bstr()
        .to_string();
    assert!(body.contains("local-change"), "ours missing: {body:?}");
    assert!(body.contains("upstream-change"), "theirs missing: {body:?}");
}

/// Upstream deleted a file the local side left untouched since the base: the
/// deletion is propagated cleanly (the path is gone, no conflict).
#[test]
fn update_propagates_upstream_deletion() {
    let upstream = tempfile::tempdir().unwrap();
    let base = upstream_two_commits(upstream.path(), &[("src/f.txt", b"v1\n")], |d| {
        std::fs::remove_file(d.join("src/f.txt")).unwrap();
    });
    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        &[("lib/f.txt", b"v1\n")],
        Some("lib/* vendor=mylib\n"),
    );
    let e = update_entry(upstream.path().to_str().unwrap(), base);
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    let merge = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");

    assert!(merge.conflicts.is_empty(), "{:?}", merge.conflicts);
    assert!(
        !paths(&repo, merge.result_tree)
            .iter()
            .any(|p| p == "lib/f.txt"),
        "upstream deletion not propagated: {:?}",
        paths(&repo, merge.result_tree),
    );
}

/// With a recorded base, `ancestor_tree` is exactly `base_tree(entry)` — i.e.
/// `upstream_tree` at the base commit — not the empty tree, not `theirs`.
#[test]
fn ancestor_tree_is_the_recorded_base_tree() {
    let upstream = tempfile::tempdir().unwrap();
    let base = upstream_two_commits(upstream.path(), &[("src/f.txt", b"v1\n")], |d| {
        write(d, "src/f.txt", b"v2\n");
    });
    let (bare, repo) = make_bare();
    let ours = commit_into(
        bare.path(),
        "refs/scratch/ours",
        &[("lib/f.txt", b"v1\n")],
        Some("lib/* vendor=mylib\n"),
    );
    let e = update_entry(upstream.path().to_str().unwrap(), base);
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");

    let merge = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");

    let base_tree = repo.base_tree(&e).expect("base_tree");
    assert!(base_tree.is_some());
    assert_eq!(merge.ancestor_tree, base_tree);
    assert_eq!(
        merge.ancestor_tree,
        Some(repo.upstream_tree(&e, base).expect("upstream_tree")),
    );
}

// ── Error propagation ─────────────────────────────────────────────────────────

/// `theirs` must resolve to a commit. Handed a blob oid, `merge_vendor` errors
/// (via `upstream_tree`) rather than panicking.
#[test]
fn non_commit_theirs_is_error() {
    let b = build(
        &[("loc/a.txt", b"A")],
        "loc/* vendor=mylib\n",
        &[("up/c.txt", b"C")],
        vec![pat("up/**", Some("vendor/"))],
    );
    let some_blob = tree_entries(
        &b.repo,
        b.repo
            .find_commit(b.ours)
            .unwrap()
            .tree_id()
            .unwrap()
            .detach(),
    )
    .into_values()
    .next()
    .expect("a blob exists");

    assert!(b.repo.merge_vendor(&b.entry, b.ours, some_blob).is_err());
}

/// `ours` must resolve to a commit. Handed a blob oid, `merge_vendor` errors
/// (via `ours_tree`) rather than panicking.
#[test]
fn non_commit_ours_is_error() {
    let b = build(
        &[("loc/a.txt", b"A")],
        "loc/* vendor=mylib\n",
        &[("up/c.txt", b"C")],
        vec![pat("up/**", Some("vendor/"))],
    );
    let some_blob = tree_entries(
        &b.repo,
        b.repo
            .find_commit(b.ours)
            .unwrap()
            .tree_id()
            .unwrap()
            .detach(),
    )
    .into_values()
    .next()
    .expect("a blob exists");

    assert!(b.repo.merge_vendor(&b.entry, some_blob, b.theirs).is_err());
}

/// A pattern that remaps to an unsafe local path is a config error, and it
/// must surface through `merge_vendor` (propagated from `upstream_tree`),
/// never silently swallowed into a content deletion.
#[test]
fn unsafe_pattern_mapping_is_config_error() {
    // `up/x.txt` with no destination: the literal prefix is the whole glob, so
    // the remapped local path is empty — rejected by the path-safety guard.
    let b = build(
        &[("loc/a.txt", b"A")],
        "loc/* vendor=mylib\n",
        &[("up/x.txt", b"C")],
        vec![pat("up/x.txt", None)],
    );

    match b.repo.merge_vendor(&b.entry, b.ours, b.theirs) {
        Err(Error::Config(_)) => {}
        other => panic!("expected Error::Config, got {other:?}"),
    }
}
