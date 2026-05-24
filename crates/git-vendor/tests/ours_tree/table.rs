//! Table-driven tests for `ours_tree`.
//!
//! `ours_tree` is a pure object-database function: given an entry and the
//! local ("ours") commit it produces the subtree of that commit's tree
//! restricted to the paths carrying the `vendor=<name>` attribute. The
//! `.gitattributes` are resolved from the *commit's tree*, never the working
//! copy, so the fixture commits a fixed layout and the cases vary only the
//! vendor name (and, to prove independence, the upstream patterns). Selection
//! preserves blob identity and mode: it is the local-path subtree, not a
//! rewrite.

use std::collections::BTreeMap;
use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorName, VendorRepository as _};
use gix::bstr::ByteSlice as _;
use rstest::rstest;

// ── Fixture helpers ───────────────────────────────────────────────────────────

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

/// Initialize a non-bare repo, run `init`, then a single `commit` of whatever
/// has been written into the working tree, and return the repo and the commit.
fn commit(dir: &Path) -> (gix::Repository, gix::ObjectId) {
    git(&["add", "-A"], dir);
    git(&["commit", "-m", "init"], dir);
    let repo = gix::open(dir).expect("gix open");
    let head = repo.head_commit().expect("head commit").id().detach();
    (repo, head)
}

fn init(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
}

/// The shared fixture: a fixed multi-directory layout whose root
/// `.gitattributes` assigns `vendor=<name>` to several paths, including a
/// last-match-wins `-vendor` override that must *exclude* a path. Returns the
/// repo and the `ours` commit.
fn fixture() -> (tempfile::TempDir, gix::Repository, gix::ObjectId) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "README.md", b"readme");
    write(p, "vendor/libfoo/a.rs", b"foo-a");
    write(p, "vendor/libfoo/sub/b.rs", b"foo-b");
    write(p, "vendor/libfoo/sub/x.rs", b"foo-x");
    write(p, "vendor/libbar/c.rs", b"bar-c");
    write(p, "shared/keep.txt", b"keep");
    write(
        p,
        ".gitattributes",
        // Gitignore-like: `*` does not cross `/`, so each directory level is
        // listed explicitly. The final line overrides the earlier libfoo match
        // for `x.rs` (last match wins), unsetting the attribute there.
        b"vendor/libfoo/*.rs vendor=libfoo\n\
          vendor/libfoo/sub/*.rs vendor=libfoo\n\
          vendor/libbar/*.rs vendor=libbar\n\
          shared/keep.txt vendor=libfoo\n\
          vendor/libfoo/sub/x.rs -vendor\n",
    );
    let (repo, head) = commit(p);
    (dir, repo, head)
}

fn entry(name: &str, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new(name).unwrap(),
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns,
    }
}

fn pat(glob: &str, destination: Option<&str>) -> PatternMapping {
    PatternMapping {
        glob: glob.to_owned(),
        destination: destination.map(str::to_owned),
    }
}

/// Flatten a tree into a fully ordered `path -> blob oid` map, skipping trees.
fn tree_entries(repo: &gix::Repository, tree: gix::ObjectId) -> BTreeMap<String, gix::ObjectId> {
    let mut out = BTreeMap::new();
    let tree = repo.find_tree(tree).expect("find tree");
    for record in tree.traverse().breadthfirst.files().expect("traverse") {
        if record.mode.is_tree() {
            continue;
        }
        out.insert(record.filepath.to_str_lossy().into_owned(), record.oid);
    }
    out
}

fn paths(repo: &gix::Repository, tree: gix::ObjectId) -> Vec<String> {
    tree_entries(repo, tree).into_keys().collect()
}

/// Result paths as raw bytes — the only faithful view when a path is not
/// UTF-8; `paths`/`tree_entries` would lossily mangle it.
#[cfg(unix)]
fn raw_paths(repo: &gix::Repository, tree: gix::ObjectId) -> Vec<gix::bstr::BString> {
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| r.filepath.clone())
        .collect()
}

// ── Git-plumbing fixtures ─────────────────────────────────────────────────────
//
// Paths with non-UTF-8 bytes cannot be written to a UTF-8-only filesystem
// (e.g. APFS), so these build the tree through the index with git plumbing —
// the same `git` CLI idiom as the rest of the file — and the path never
// touches disk.

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

/// `git hash-object -w -t <kind>`, content piped on stdin; returns the hex oid.
fn hash_object(dir: &Path, kind: &str, content: &[u8]) -> String {
    use std::io::Write as _;
    let mut child = std::process::Command::new("git")
        .args(["hash-object", "-w", "-t", kind, "--stdin"])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("git hash-object");
    child.stdin.take().unwrap().write_all(content).unwrap();
    let out = child.wait_with_output().expect("hash-object");
    assert!(out.status.success(), "hash-object failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Stage one entry into the index at a raw-byte `path` — `update-index
/// --cacheinfo` takes an index path, not a filesystem path.
#[cfg(unix)]
fn cacheinfo(dir: &Path, mode: &str, oid: &str, path: &[u8]) {
    use std::os::unix::ffi::OsStrExt as _;
    let mut spec = format!("{mode},{oid},").into_bytes();
    spec.extend_from_slice(path);
    let status = std::process::Command::new("git")
        .arg("update-index")
        .arg("--add")
        .arg("--cacheinfo")
        .arg(std::ffi::OsStr::from_bytes(&spec))
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .expect("git update-index");
    assert!(status.success(), "update-index --cacheinfo {mode} failed");
}

/// Write the current index as a tree and wrap it in a (dangling) commit.
fn plumbing_commit(dir: &Path) -> (gix::Repository, gix::ObjectId) {
    let tree = String::from_utf8(git_capture(&["write-tree"], dir)).unwrap();
    let commit = String::from_utf8(git_capture(
        &["commit-tree", tree.trim(), "-m", "init"],
        dir,
    ))
    .unwrap();
    let repo = gix::open(dir).expect("gix open");
    let oid = gix::ObjectId::from_hex(commit.trim().as_bytes()).expect("commit oid");
    (repo, oid)
}

// ── Attribute-driven selection ────────────────────────────────────────────────

/// The vendor name picks exactly the paths whose `vendor` attribute resolves to
/// that name. A name that no `.gitattributes` rule yields selects nothing (the
/// empty tree, not an error). `vendor/libfoo/sub/x.rs` is absent from the
/// libfoo set: the trailing `-vendor` rule wins over the earlier match.
#[rstest]
#[case(
    "libfoo",
    &["shared/keep.txt", "vendor/libfoo/a.rs", "vendor/libfoo/sub/b.rs"]
)]
#[case("libbar", &["vendor/libbar/c.rs"])]
#[case("ghost", &[])]
fn name_selects_attributed_paths(#[case] name: &str, #[case] expected: &[&str]) {
    let (_dir, repo, ours) = fixture();
    let entry = entry(name, vec![]);

    let tree = repo.ours_tree(&entry, ours).expect("ours_tree");

    assert_eq!(paths(&repo, tree), expected);
}

/// An unmatched name yields the canonical empty tree object, byte-identical to
/// `repo.empty_tree()`, not merely an entry-free tree.
#[test]
fn unmatched_name_is_the_empty_tree() {
    let (_dir, repo, ours) = fixture();
    let entry = entry("ghost", vec![]);

    let tree = repo.ours_tree(&entry, ours).expect("ours_tree");

    assert_eq!(tree, repo.empty_tree().id().detach());
}

// ── Independence from the upstream pattern filter ─────────────────────────────

/// `ours_tree` is the local-side filter (`.gitattributes`), wholly independent
/// of the entry's upstream `PatternMapping`s. Pattern globs and destinations
/// that would reshape `upstream_tree` leave `ours_tree` byte-identical.
#[rstest]
#[case(vec![])]
#[case(vec![pat("**", Some("anywhere/"))])]
#[case(vec![pat("vendor/libbar/*.rs", None), pat("nope/**", Some("x/"))])]
fn ignores_upstream_patterns(#[case] patterns: Vec<PatternMapping>) {
    let (_dir, repo, ours) = fixture();

    let baseline = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");
    let with_patterns = repo
        .ours_tree(&entry("libfoo", patterns), ours)
        .expect("ours_tree");

    assert_eq!(with_patterns, baseline);
}

// ── Content preservation ──────────────────────────────────────────────────────

/// Selection preserves blob identity: a selected path maps to the exact blob
/// object the `ours` commit holds at that same path (same oid, no rewrite, no
/// remap — local path space in, local path space out).
#[test]
fn selection_preserves_blob_identity() {
    let (_dir, repo, ours) = fixture();
    let ours_full = tree_entries(
        &repo,
        repo.find_commit(ours).unwrap().tree_id().unwrap().detach(),
    );

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");
    let result = tree_entries(&repo, tree);

    assert!(!result.is_empty());
    for (path, oid) in &result {
        assert_eq!(Some(oid), ours_full.get(path), "blob changed at {path}");
    }
}

/// Selection preserves the entry mode: an upstream-of-local executable stays
/// executable at its selected path.
#[test]
fn selection_preserves_executable_mode() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "tools/run.sh", b"#!/bin/sh\n");
    write(p, ".gitattributes", b"tools/run.sh vendor=libfoo\n");
    git(&["add", "-A"], p);
    git(&["update-index", "--chmod=+x", "tools/run.sh"], p);
    git(&["commit", "-m", "init"], p);
    let repo = gix::open(p).expect("gix open");
    let ours = repo.head_commit().expect("head commit").id().detach();

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");
    let record = repo
        .find_tree(tree)
        .expect("find tree")
        .traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .find(|r| r.filepath.to_str_lossy() == "tools/run.sh")
        .expect("selected path present");

    assert!(
        record.mode.is_executable(),
        "executable bit must survive selection, got mode {:?}",
        record.mode,
    );
}

// ── Attribute resolution edge cases ───────────────────────────────────────────

/// A repo with no `.gitattributes` at all carries the attribute nowhere: every
/// name selects the empty tree.
#[test]
fn no_gitattributes_selects_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "src/a.rs", b"a");
    write(p, "src/b.rs", b"b");
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(tree, repo.empty_tree().id().detach());
}

/// `.gitattributes` is resolved from the *tree*, honoring nested files at any
/// depth — not just the repo-root one. A rule in `nested/.gitattributes`
/// selects a sibling blob, proving the resolver descends the tree rather than
/// reading a single root file or the working copy.
#[test]
fn nested_gitattributes_is_honored() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "nested/data.bin", b"\x00\x01");
    write(p, "nested/other.bin", b"\x02");
    write(p, "nested/.gitattributes", b"data.bin vendor=libfoo\n");
    write(p, "top.txt", b"top");
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(paths(&repo, tree), &["nested/data.bin"]);
}

/// Cross-file precedence: a deeper `pkg/.gitattributes` overrides a root rule
/// for the same path, exactly as git resolves it. The root selects every
/// `pkg/*.rs`; the nested file unsets `drop.rs`, so only `keep.rs` survives.
/// This pins the documented "nested … precedence honored as git resolves
/// them" claim across files, not merely within one.
#[test]
fn nested_gitattributes_overrides_root() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "pkg/keep.rs", b"keep");
    write(p, "pkg/drop.rs", b"drop");
    write(p, ".gitattributes", b"pkg/*.rs vendor=libfoo\n");
    write(p, "pkg/.gitattributes", b"drop.rs -vendor\n");
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(paths(&repo, tree), &["pkg/keep.rs"]);
}

/// Selection preserves a symlink entry as a symlink: the link path carries the
/// attribute and is `upsert`ed with its link mode and the exact blob (the
/// link's target bytes), not dereferenced or rewritten.
#[cfg(unix)]
#[test]
fn selection_preserves_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "real.txt", b"real");
    std::os::unix::fs::symlink("real.txt", p.join("link")).unwrap();
    write(p, ".gitattributes", b"link vendor=libfoo\n");
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");
    let record = repo
        .find_tree(tree)
        .expect("find tree")
        .traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .find(|r| r.filepath.to_str_lossy() == "link")
        .expect("selected link present");

    assert!(
        record.mode.is_link(),
        "symlink mode must survive selection, got mode {:?}",
        record.mode,
    );
}

/// Only an explicit `vendor=<name>` value selects. A bare `vendor` (state
/// `Set`) and an unset `-vendor` (state `Unset`) both carry the *attribute*
/// but no value, so neither is selected for `name`.
#[test]
fn bare_and_unset_attribute_are_not_selected() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "set.rs", b"set");
    write(p, "unset.rs", b"unset");
    write(p, "valued.rs", b"valued");
    write(
        p,
        ".gitattributes",
        b"set.rs vendor\nunset.rs -vendor\nvalued.rs vendor=libfoo\n",
    );
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(paths(&repo, tree), &["valued.rs"]);
}

/// A different value on the same attribute does not bleed across vendors: a
/// path valued `vendor=libbar` is invisible to `libfoo` and vice versa.
#[test]
fn distinct_values_do_not_cross_select() {
    let (_dir, repo, ours) = fixture();

    let foo = paths(
        &repo,
        repo.ours_tree(&entry("libfoo", vec![]), ours)
            .expect("ours_tree"),
    );
    let bar = paths(
        &repo,
        repo.ours_tree(&entry("libbar", vec![]), ours)
            .expect("ours_tree"),
    );

    assert!(foo.iter().all(|p| !bar.contains(p)));
    assert!(bar.iter().all(|p| !foo.contains(p)));
}

// ── Error paths ───────────────────────────────────────────────────────────────

/// `ours` must resolve to a commit. Handed a blob oid, `ours_tree` errors
/// rather than panicking.
#[test]
fn non_commit_ours_is_error() {
    let (_dir, repo, ours) = fixture();
    let blob = tree_entries(
        &repo,
        repo.find_commit(ours).unwrap().tree_id().unwrap().detach(),
    )
    .into_values()
    .next()
    .expect("a blob exists");

    assert!(repo.ours_tree(&entry("libfoo", vec![]), blob).is_err());
}

/// A well-formed but absent oid is an error, not a panic — distinct from the
/// wrong-kind (`non_commit_ours_is_error`) case.
#[test]
fn missing_object_ours_is_error() {
    let (_dir, repo, _ours) = fixture();
    let bogus =
        gix::ObjectId::from_hex(b"ffffffffffffffffffffffffffffffffffffffff").expect("hex oid");

    assert!(repo.ours_tree(&entry("libfoo", vec![]), bogus).is_err());
}

// ── Raw-byte path fidelity (the lossy-UTF-8 hazard) ───────────────────────────

/// A non-UTF-8 path carrying the attribute round-trips byte-for-byte: same
/// path bytes, same blob oid, no `U+FFFD` substitution. This is the path
/// `to_str_lossy` would silently rename in the result tree.
#[cfg(unix)]
#[test]
fn non_utf8_path_preserved_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, ".gitattributes", b"bin/* vendor=libfoo\n");
    git(&["add", ".gitattributes"], p);
    let blob = hash_object(p, "blob", b"weird");
    cacheinfo(p, "100644", &blob, b"bin/\xff\xfe.rs");
    let (repo, ours) = plumbing_commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(
        raw_paths(&repo, tree),
        vec![gix::bstr::BString::from(&b"bin/\xff\xfe.rs"[..])],
    );
    let record = repo
        .find_tree(tree)
        .unwrap()
        .traverse()
        .breadthfirst
        .files()
        .unwrap()
        .into_iter()
        .find(|r| !r.mode.is_tree())
        .unwrap();
    assert_eq!(
        record.oid,
        gix::ObjectId::from_hex(blob.as_bytes()).unwrap(),
        "blob must be preserved, not rewritten",
    );
}

/// The precise regression for the fixed bug: two sibling paths that collapse
/// to the same string under lossy UTF-8, only one of which is attributed. The
/// lossy implementation would (a) admit the unattributed sibling via the
/// collapsed selection key and (b) collide them in the editor. Exactly the
/// attributed path, by its real bytes, must survive — and only it.
#[cfg(unix)]
#[test]
fn lossy_collision_does_not_misselect() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    // Literal (wildcard-free) rule: only the `\xff` sibling is attributed,
    // yet `x/\xff` and `x/\xfe` both lossy-decode to `x/<U+FFFD>`.
    write(p, ".gitattributes", b"x/\xff vendor=libfoo\n");
    git(&["add", ".gitattributes"], p);
    let selected = hash_object(p, "blob", b"selected");
    let other = hash_object(p, "blob", b"unselected");
    cacheinfo(p, "100644", &selected, b"x/\xff");
    cacheinfo(p, "100644", &other, b"x/\xfe");
    let (repo, ours) = plumbing_commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(
        raw_paths(&repo, tree),
        vec![gix::bstr::BString::from(&b"x/\xff"[..])],
    );
}

// ── Non-blob entry kinds ──────────────────────────────────────────────────────

/// A submodule (gitlink) entry carrying the attribute is selected and keeps
/// its commit mode; it is not a tree, so the `is_tree` skip must not drop it,
/// and its target commit need not be present.
#[test]
fn selection_preserves_gitlink() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, ".gitattributes", b"sub vendor=libfoo\n");
    git(&["add", ".gitattributes"], p);
    let empty_tree = hash_object(p, "tree", b"");
    let sub = String::from_utf8(git_capture(&["commit-tree", &empty_tree, "-m", "sub"], p))
        .unwrap()
        .trim()
        .to_owned();
    git(
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{sub},sub"),
        ],
        p,
    );
    let (repo, ours) = plumbing_commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");
    let record = repo
        .find_tree(tree)
        .expect("find tree")
        .traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .find(|r| r.filepath.to_str_lossy() == "sub")
        .expect("gitlink present");

    assert!(
        record.mode.is_commit(),
        "gitlink mode must survive selection, got mode {:?}",
        record.mode,
    );
}

// ── Attribute resolution: values and spacing ──────────────────────────────────

/// Last-match-wins applies to the *value*, not only set/unset: a path
/// re-valued `vendor=libbar` after `vendor=libfoo` selects for `libbar` only.
#[test]
fn value_reassignment_last_match_wins() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "f.rs", b"f");
    write(
        p,
        ".gitattributes",
        b"f.rs vendor=libfoo\nf.rs vendor=libbar\n",
    );
    let (repo, ours) = commit(p);

    let foo = paths(
        &repo,
        repo.ours_tree(&entry("libfoo", vec![]), ours)
            .expect("ours_tree"),
    );
    let bar = paths(
        &repo,
        repo.ours_tree(&entry("libbar", vec![]), ours)
            .expect("ours_tree"),
    );

    assert!(foo.is_empty(), "superseded value still selected: {foo:?}");
    assert_eq!(bar, &["f.rs"]);
}

/// A path containing a space is selected via a quoted `.gitattributes`
/// pattern. The `git check-attr` oracle parses on `": "` and so cannot cover
/// this; it is pinned here directly.
#[test]
fn quoted_pattern_with_space_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a dir/a file.rs", b"x");
    write(p, ".gitattributes", b"\"a dir/a file.rs\" vendor=libfoo\n");
    let (repo, ours) = commit(p);

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(paths(&repo, tree), &["a dir/a file.rs"]);
}

/// An `ours` commit whose tree is empty selects the canonical empty tree —
/// `index_from_tree` of an empty tree must not error.
#[test]
fn empty_ours_tree_selects_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    let empty_tree = hash_object(p, "tree", b"");
    let commit = String::from_utf8(git_capture(&["commit-tree", &empty_tree, "-m", "init"], p))
        .unwrap()
        .trim()
        .to_owned();
    let repo = gix::open(p).expect("gix open");
    let ours = gix::ObjectId::from_hex(commit.as_bytes()).expect("commit oid");

    let tree = repo
        .ours_tree(&entry("libfoo", vec![]), ours)
        .expect("ours_tree");

    assert_eq!(tree, repo.empty_tree().id().detach());
}
