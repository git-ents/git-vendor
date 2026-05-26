//! Table-driven tests for `vendor_paths`.
//!
//! `vendor_paths` is the local-side content filter: the list of paths in the
//! `ours` commit's tree whose `.gitattributes` `vendor` attribute resolves to
//! the entry name, resolved from the *tree* (never the working copy). It is
//! the path-list counterpart of `ours_tree` — the same selection, before the
//! subtree is built — so the fixture commits a fixed layout and the cases vary
//! only the vendor name.

use std::collections::BTreeSet;
use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
use gix::bstr::BString;
use rstest::rstest;

use crate::support::{commit, git, init, write};

/// The shared fixture: a fixed layout whose root `.gitattributes` assigns
/// `vendor=<name>` to several paths, with a trailing `-vendor` rule that must
/// *exclude* a path (last match wins). Returns the repo and the `ours` commit.
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
        mode: VendorMode::Merge,
    }
}

fn pat(glob: &str, destination: Option<&str>) -> PatternMapping {
    PatternMapping {
        glob: glob.to_owned(),
        destination: destination.map(str::to_owned),
    }
}

/// `vendor_paths` returns a list; the contract does not fix its order, so
/// compare as a set.
fn sorted(mut v: Vec<BString>) -> Vec<BString> {
    v.sort();
    v
}

fn ours_tree_paths(repo: &gix::Repository, tree: gix::ObjectId) -> BTreeSet<BString> {
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| r.filepath)
        .collect()
}

// ── Attribute-driven selection ────────────────────────────────────────────────

/// The vendor name picks exactly the paths whose `vendor` attribute resolves to
/// that name. A name no rule yields selects nothing. `vendor/libfoo/sub/x.rs`
/// is excluded from libfoo: the trailing `-vendor` rule wins.
#[rstest]
#[case(
    "libfoo",
    &["shared/keep.txt", "vendor/libfoo/a.rs", "vendor/libfoo/sub/b.rs"]
)]
#[case("libbar", &["vendor/libbar/c.rs"])]
#[case("ghost", &[])]
fn name_selects_attributed_paths(#[case] name: &str, #[case] expected: &[&str]) {
    let (_dir, repo, ours) = fixture();

    let paths = repo
        .vendor_paths(&entry(name, vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(
        sorted(paths),
        expected
            .iter()
            .map(|&s| BString::from(s))
            .collect::<Vec<_>>()
    );
}

// ── Relationship to `ours_tree` ───────────────────────────────────────────────

/// The documented contract: `ours_tree` is the `ours` commit's tree
/// "restricted to `vendor_paths`". The path set of the `ours_tree` result must
/// therefore equal the `vendor_paths` list exactly, for the same entry.
#[rstest]
#[case("libfoo")]
#[case("libbar")]
#[case("ghost")]
fn agrees_with_ours_tree(#[case] name: &str) {
    let (_dir, repo, ours) = fixture();
    let entry = entry(name, vec![]);

    let listed: BTreeSet<BString> = repo
        .vendor_paths(&entry, ours)
        .expect("vendor_paths")
        .into_iter()
        .collect();
    let tree = repo.ours_tree(&entry, ours).expect("ours_tree");

    assert_eq!(listed, ours_tree_paths(&repo, tree));
}

// ── Independence from the upstream pattern filter ─────────────────────────────

/// `vendor_paths` is the local-side filter (`.gitattributes`), independent of
/// the entry's upstream `PatternMapping`s. Pattern globs and destinations leave
/// the list unchanged.
#[rstest]
#[case(vec![])]
#[case(vec![pat("**", Some("anywhere/"))])]
#[case(vec![pat("vendor/libbar/*.rs", None), pat("nope/**", Some("x/"))])]
fn ignores_upstream_patterns(#[case] patterns: Vec<PatternMapping>) {
    let (_dir, repo, ours) = fixture();

    let baseline = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");
    let with_patterns = repo
        .vendor_paths(&entry("libfoo", patterns), ours)
        .expect("vendor_paths");

    assert_eq!(with_patterns, baseline);
}

// ── Attribute resolution edge cases ───────────────────────────────────────────

/// A repo with no `.gitattributes` carries the attribute nowhere: every name
/// yields the empty list.
#[test]
fn no_gitattributes_selects_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "src/a.rs", b"a");
    let (repo, ours) = commit(p);

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert!(paths.is_empty());
}

/// `.gitattributes` is resolved from the tree at any depth: a rule in
/// `nested/.gitattributes` selects a sibling, proving the resolver descends
/// the tree rather than reading only the repo-root file or the working copy.
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

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("nested/data.bin")]);
}

/// Only an explicit `vendor=<name>` value selects: a bare `vendor` (state
/// `Set`) and an unset `-vendor` (state `Unset`) carry the attribute but no
/// value, so neither is listed.
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

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("valued.rs")]);
}

/// A different value on the same attribute does not bleed across vendors.
#[test]
fn distinct_values_do_not_cross_select() {
    let (_dir, repo, ours) = fixture();

    let foo = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");
    let bar = repo
        .vendor_paths(&entry("libbar", vec![]), ours)
        .expect("vendor_paths");

    assert!(foo.iter().all(|p| !bar.contains(p)));
    assert!(bar.iter().all(|p| !foo.contains(p)));
}

// ── Error paths ───────────────────────────────────────────────────────────────

/// `ours` must resolve to a commit. Handed a blob oid, `vendor_paths` errors
/// rather than panicking.
#[test]
fn non_commit_ours_is_error() {
    let (_dir, repo, ours) = fixture();
    let blob = repo
        .find_commit(ours)
        .unwrap()
        .tree()
        .unwrap()
        .traverse()
        .breadthfirst
        .files()
        .unwrap()
        .into_iter()
        .find(|r| !r.mode.is_tree())
        .expect("a blob exists")
        .oid;

    assert!(repo.vendor_paths(&entry("libfoo", vec![]), blob).is_err());
}

// ── BString / path encoding ───────────────────────────────────────────────────

/// A filename containing ASCII space bytes is returned as a BString with the
/// space intact — no escaping or truncation.
#[test]
fn path_with_spaces_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "sub/file with spaces.txt", b"content");
    write(p, ".gitattributes", b"*.txt vendor=libfoo\n");
    let (repo, ours) = commit(p);

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("sub/file with spaces.txt")]);
}

/// A filename containing non-ASCII Unicode (valid UTF-8) is returned verbatim;
/// the BString bytes equal the original UTF-8 encoding.
#[test]
fn unicode_filename_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "data/日本語.bin", b"content");
    write(p, ".gitattributes", b"*.bin vendor=libfoo\n");
    let (repo, ours) = commit(p);

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("data/日本語.bin")]);
}

/// A non-UTF-8 filename is preserved exactly in the returned BString — the bytes
/// are not sanitized or replaced. `to_str_lossy` would corrupt them, confirming
/// BString is the right return type.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filename_is_preserved() {
    use gix::bstr::ByteSlice as _;
    use std::os::unix::ffi::OsStrExt as _;

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    let raw: &[u8] = b"file\xff.bin";
    std::fs::write(p.join(std::ffi::OsStr::from_bytes(raw)), b"content")
        .expect("write non-UTF-8 file");
    write(p, ".gitattributes", b"*.bin vendor=libfoo\n");
    let (repo, ours) = commit(p);

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].as_bytes(), raw);
}

// ── Attribute-file precedence ─────────────────────────────────────────────────

/// A child `.gitattributes` that unsets `vendor` beats the parent's assignment:
/// the path no longer appears in the result.
#[test]
fn child_gitattributes_overrides_parent() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "sub/a.rs", b"keep");
    write(p, "sub/inner/b.rs", b"drop");
    write(p, ".gitattributes", b"sub/**/*.rs vendor=libfoo\n");
    write(p, "sub/inner/.gitattributes", b"*.rs -vendor\n");
    let (repo, ours) = commit(p);

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("sub/a.rs")]);
}

/// `.gitattributes` is a regular blob in the tree and obeys the same selection
/// rule as any other file: if it carries `vendor=<name>`, it appears in the
/// result.
#[test]
fn gitattributes_file_itself_is_selectable() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "src/lib.rs", b"lib");
    write(
        p,
        ".gitattributes",
        b".gitattributes vendor=libfoo\nsrc/lib.rs vendor=libfoo\n",
    );
    let (repo, ours) = commit(p);

    let mut paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");
    paths.sort();

    assert_eq!(
        paths,
        &[BString::from(".gitattributes"), BString::from("src/lib.rs")]
    );
}

// ── Resolution source: tree, never the working copy ───────────────────────────

/// The decisive `Source::IdMapping` guarantee: attributes are resolved from the
/// `ours` commit's tree, never the working copy. Commit one assignment, then
/// diverge the *uncommitted* working-copy `.gitattributes` to a different
/// vendor; the committed value must still win and the divergent one select
/// nothing.
#[test]
fn worktree_gitattributes_does_not_override_tree() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(p, ".gitattributes", b"a.rs vendor=libfoo\n");
    let (repo, ours) = commit(p);

    // Diverge the working copy without committing it.
    write(p, ".gitattributes", b"a.rs vendor=libbar\n");

    let foo = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");
    let bar = repo
        .vendor_paths(&entry("libbar", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(foo, &[BString::from("a.rs")]);
    assert!(
        bar.is_empty(),
        "uncommitted worktree attribute leaked: {bar:?}"
    );
}

// ── `ours_tree` byte round-trip ───────────────────────────────────────────────

/// A non-UTF-8 path must round-trip through `ours_tree`
/// byte-for-byte. A lossy conversion anywhere on the select-then-build path
/// would rename or drop it; this is what `BString` (over `String`) buys.
#[cfg(target_os = "linux")]
#[test]
fn ours_tree_round_trips_non_utf8_path() {
    use gix::bstr::ByteSlice as _;
    use std::os::unix::ffi::OsStrExt as _;

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    let raw: &[u8] = b"file\xff.bin";
    std::fs::write(p.join(std::ffi::OsStr::from_bytes(raw)), b"content")
        .expect("write non-UTF-8 file");
    write(p, ".gitattributes", b"*.bin vendor=libfoo\n");
    let (repo, ours) = commit(p);
    let e = entry("libfoo", vec![]);

    let listed: BTreeSet<BString> = repo
        .vendor_paths(&e, ours)
        .expect("vendor_paths")
        .into_iter()
        .collect();
    let tree = repo.ours_tree(&e, ours).expect("ours_tree");
    let in_tree = ours_tree_paths(&repo, tree);

    assert_eq!(listed, in_tree);
    assert!(in_tree.iter().any(|p| p.as_bstr() == raw.as_bstr()));
}

// ── Non-blob tree entries ─────────────────────────────────────────────────────

/// A symlink entry is neither a tree nor a regular blob; the resolver's only
/// skip is `is_tree()`, so a symlink carrying `vendor=<name>` must be selected
/// and must round-trip through `ours_tree` with its symlink mode intact.
#[cfg(unix)]
#[test]
fn symlink_entry_is_selected_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "target.txt", b"t");
    std::os::unix::fs::symlink("target.txt", p.join("link")).expect("symlink");
    write(p, ".gitattributes", b"link vendor=libfoo\n");
    let (repo, ours) = commit(p);
    let e = entry("libfoo", vec![]);

    let paths = repo.vendor_paths(&e, ours).expect("vendor_paths");
    assert_eq!(paths, &[BString::from("link")]);

    let tree = repo.ours_tree(&e, ours).expect("ours_tree");
    assert!(ours_tree_paths(&repo, tree).contains(&BString::from("link")));
}

/// A gitlink (submodule) entry has commit mode `160000`: not a tree, so the
/// resolver processes it. With a rule assigning the attribute to its path it
/// is selected like any other entry.
#[test]
fn gitlink_entry_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, ".gitattributes", b"sub vendor=libfoo\n");
    git(&["add", ".gitattributes"], p);
    let fake_commit = "deadbeef".repeat(5); // 40 hex; gitlink target need not exist
    git(
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{fake_commit},sub"),
        ],
        p,
    );
    git(&["commit", "-m", "init"], p);
    let repo = gix::open(p).expect("gix open");
    let ours = repo.head_commit().expect("head commit").id().detach();

    let paths = repo
        .vendor_paths(&entry("libfoo", vec![]), ours)
        .expect("vendor_paths");

    assert_eq!(paths, &[BString::from("sub")]);
}

// ── `ours` must be a commit: every non-commit oid ─────────────────────────────

fn git_stdout(args: &[&str], dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?} failed in {dir:?}");
    String::from_utf8(out.stdout).expect("utf-8 git output")
}

/// A tree oid is not a commit: `vendor_paths` errors rather than panicking.
#[test]
fn tree_oid_ours_is_error() {
    let (_dir, repo, ours) = fixture();
    let tree = repo
        .find_commit(ours)
        .unwrap()
        .tree()
        .unwrap()
        .id()
        .detach();

    assert!(repo.vendor_paths(&entry("libfoo", vec![]), tree).is_err());
}

/// An annotated-tag oid (the tag object itself, unpeeled) is not a commit:
/// `vendor_paths` errors rather than silently resolving the tag's target.
#[test]
fn annotated_tag_oid_ours_is_error() {
    let (dir, repo, _ours) = fixture();
    let p = dir.path();
    git(&["tag", "-a", "v1", "-m", "tag"], p);
    let hex = git_stdout(&["rev-parse", "refs/tags/v1"], p);
    let tag = gix::ObjectId::from_hex(hex.trim().as_bytes()).expect("tag oid");

    assert!(repo.vendor_paths(&entry("libfoo", vec![]), tag).is_err());
}

/// An oid absent from the object database errors rather than panicking.
#[test]
fn nonexistent_oid_ours_is_error() {
    let (_dir, repo, _ours) = fixture();
    let missing =
        gix::ObjectId::from_hex(b"0123456789abcdef0123456789abcdef01234567").expect("hex oid");

    assert!(
        repo.vendor_paths(&entry("libfoo", vec![]), missing)
            .is_err()
    );
}

// ── Last-match precedence, both directions ────────────────────────────────────
//
// The tests in this section and the two below (attribute macros, parsing
// robustness) are dependency-characterization pins, not coverage of our
// resolver: the selection outcome here is entirely decided by gix's attribute
// engine, which `resolve_vendor_paths` only consumes. They pass because gix is
// git-compatible; their purpose is to fail loudly if a gix upgrade ever
// regresses that compatibility, since our `Source::IdMapping` path would
// silently inherit the regression.

/// Within one `.gitattributes`, the last matching rule's value wins: a path
/// reassigned `libfoo` then `libbar` belongs only to `libbar`.
#[test]
fn last_value_in_same_file_wins() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(
        p,
        ".gitattributes",
        b"a.rs vendor=libfoo\na.rs vendor=libbar\n",
    );
    let (repo, ours) = commit(p);

    assert!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths")
            .is_empty()
    );
    assert_eq!(
        repo.vendor_paths(&entry("libbar", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

/// The reverse of the fixture's unset-last case: `-vendor` followed by a value
/// re-selects the path (last match wins, symmetrically).
#[test]
fn unset_then_value_reselects() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(p, ".gitattributes", b"a.rs -vendor\na.rs vendor=libfoo\n");
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

// ── Attribute macros ──────────────────────────────────────────────────────────

/// A top-level `[attr]` macro expanding to `vendor=<name>` selects the paths
/// it is applied to, exactly as git's own attribute machinery expands it.
#[test]
fn attr_macro_is_expanded() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(
        p,
        ".gitattributes",
        b"[attr]vend vendor=libfoo\n*.rs vend\n",
    );
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

// ── Attribute-value comparison is exact ───────────────────────────────────────

/// The value match is a byte-exact comparison: `vendor=LibFoo` is not selected
/// by name `libfoo`, and is selected by name `LibFoo`.
#[test]
fn attribute_value_is_case_sensitive() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(p, ".gitattributes", b"a.rs vendor=LibFoo\n");
    let (repo, ours) = commit(p);

    assert!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths")
            .is_empty()
    );
    assert_eq!(
        repo.vendor_paths(&entry("LibFoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

// ── `.gitattributes` parsing robustness ───────────────────────────────────────

/// CRLF-terminated `.gitattributes` lines (Windows-authored) resolve the same
/// as LF: the trailing `\r` is not taken as part of the value.
#[test]
fn crlf_gitattributes_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(p, ".gitattributes", b"a.rs vendor=libfoo\r\n");
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

/// A final rule with no trailing newline still applies.
#[test]
fn gitattributes_without_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"a");
    write(p, ".gitattributes", b"a.rs vendor=libfoo");
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("a.rs")]
    );
}

/// A C-quoted pathname pattern (git's syntax for spaces/specials in the
/// pattern itself) matches the corresponding path.
#[test]
fn quoted_pattern_with_spaces_matches() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "file with spaces.txt", b"content");
    write(
        p,
        ".gitattributes",
        b"\"file with spaces.txt\" vendor=libfoo\n",
    );
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("file with spaces.txt")]
    );
}

/// A leading `/` anchors a pattern to the directory of its `.gitattributes`
/// file: `/a.rs` in `sub/.gitattributes` matches `sub/a.rs` only, not a
/// same-named file at the root.
#[test]
fn leading_slash_anchors_to_attributes_dir() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    write(p, "a.rs", b"root");
    write(p, "sub/a.rs", b"sub");
    write(p, "sub/.gitattributes", b"/a.rs vendor=libfoo\n");
    let (repo, ours) = commit(p);

    assert_eq!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths"),
        &[BString::from("sub/a.rs")]
    );
}

// ── Degenerate trees ──────────────────────────────────────────────────────────

/// An empty-tree commit (no entries at all) selects nothing for any name.
#[test]
fn empty_tree_selects_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    init(p);
    git(&["commit", "--allow-empty", "-m", "init"], p);
    let repo = gix::open(p).expect("gix open");
    let ours = repo.head_commit().expect("head commit").id().detach();

    assert!(
        repo.vendor_paths(&entry("libfoo", vec![]), ours)
            .expect("vendor_paths")
            .is_empty()
    );
}
