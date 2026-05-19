//! Table-driven tests for `upstream_tree`.
//!
//! `upstream_tree` is a pure object-database function: given an entry and a
//! commit it produces a tree in *local path space*, selecting upstream blobs
//! by the entry's globs and remapping their paths via each
//! [`PatternMapping`]'s literal prefix and destination. The fixture builds one
//! upstream history with a fixed layout, fetches it into a bare local repo so
//! the objects are in the local odb, and the cases vary only the patterns.

use std::collections::BTreeMap;
use std::path::Path;

use git_vendor::{Error, PatternMapping, VendorEntry, VendorName, VendorRepository as _};
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

/// Initialize an upstream repo with a fixed multi-directory layout on `main`.
fn make_upstream(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join("README.md"), b"readme").unwrap();
    std::fs::create_dir_all(dir.join("src/sub")).unwrap();
    std::fs::write(dir.join("src/a.rs"), b"a").unwrap();
    std::fs::write(dir.join("src/sub/b.rs"), b"b").unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/c.rs"), b"c").unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-m", "init"], dir);
}

/// Initialize a bare local repo that will fetch from upstream.
fn make_local(dir: &Path) -> gix::Repository {
    git(&["init", "--bare", "-b", "main"], dir);
    gix::open(dir).expect("gix open")
}

fn entry(url: &str, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
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

/// The fixture: an upstream populated and fetched into a fresh local repo,
/// returning the local repo and the fetched upstream tip commit.
fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    gix::Repository,
    gix::ObjectId,
) {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let probe = entry(upstream.path().to_str().unwrap(), vec![]);
    let tip = repo.fetch_vendor(&probe).expect("fetch_vendor");
    (upstream, local, repo, tip)
}

/// Flatten a tree object into a `local path -> blob oid` map, skipping trees.
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

// ── Single-pattern selection and remapping ────────────────────────────────────

/// One pattern selects a subset of the upstream blobs and rewrites their paths:
/// the glob's literal prefix is stripped, then the destination (if any) is
/// prepended. The result is the set of local paths, fully ordered.
#[rstest]
// `src/**`: literal prefix `src/` is stripped, no destination.
#[case("src/**", None, &["a.rs", "sub/b.rs"])]
// Same selection, remapped under a destination prefix.
#[case("src/**", Some("vendor/"), &["vendor/a.rs", "vendor/sub/b.rs"])]
// A leaf directory.
#[case("lib/**", Some("ext/"), &["ext/c.rs"])]
// `**` selects everything; empty literal prefix, so paths are mirrored verbatim
// under the destination.
#[case(
    "**",
    Some("third_party/"),
    &[
        "third_party/README.md",
        "third_party/lib/c.rs",
        "third_party/src/a.rs",
        "third_party/src/sub/b.rs",
    ]
)]
// Gitignore-like semantics: `*` does not cross `/`, so `src/*.rs` selects
// only the direct child `src/a.rs`, NOT the nested `src/sub/b.rs`. This pins
// the `literal_separator(true)` fix; under globset's default it would also
// match `src/sub/b.rs`. The literal prefix `src/` is stripped.
#[case("src/*.rs", None, &["a.rs"])]
// `*.md` matches the top-level `.md` blob only; a nested `docs/x.md` would
// not match, by the same non-crossing behavior.
#[case("*.md", None, &["README.md"])]
// A glob that matches nothing yields the empty tree (no entries).
#[case("nope/**", None, &[])]
fn single_pattern_selects_and_remaps(
    #[case] glob: &str,
    #[case] destination: Option<&str>,
    #[case] expected: &[&str],
) {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry("unused", vec![pat(glob, destination)]);

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");

    assert_eq!(paths(&repo, tree), expected);
}

// ── Multi-pattern behavior ────────────────────────────────────────────────────

/// When several patterns match the same upstream path, the first one in entry
/// order wins, so its destination is the one applied.
#[test]
fn first_matching_pattern_wins() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry(
        "unused",
        vec![
            pat("src/**", Some("first/")),
            pat("src/**", Some("second/")),
        ],
    );

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");

    assert_eq!(paths(&repo, tree), &["first/a.rs", "first/sub/b.rs"]);
}

/// Disjoint patterns union their selections into one local tree.
#[test]
fn disjoint_patterns_union() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry(
        "unused",
        vec![pat("src/**", Some("s/")), pat("lib/**", Some("l/"))],
    );

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");

    assert_eq!(paths(&repo, tree), &["l/c.rs", "s/a.rs", "s/sub/b.rs"]);
}

/// No patterns means nothing is selected: the result is the empty tree, not an
/// error and not the upstream tree.
#[test]
fn no_patterns_yields_empty_tree() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry("unused", vec![]);

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");

    assert_eq!(tree, repo.empty_tree().id().detach());
}

// ── Content preservation ──────────────────────────────────────────────────────

/// Remapping changes paths only: the blob behind a remapped local path is the
/// exact upstream blob object, byte-for-byte (same oid).
#[test]
fn remapping_preserves_blob_identity() {
    let (_up, _local, repo, tip) = fixture();

    let upstream = tree_entries(
        &repo,
        repo.find_commit(tip).unwrap().tree_id().unwrap().detach(),
    );
    let entry = entry("unused", vec![pat("src/**", Some("vendor/"))]);

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");
    let result = tree_entries(&repo, tree);

    assert_eq!(
        result[&"vendor/a.rs".to_owned()],
        upstream[&"src/a.rs".to_owned()]
    );
    assert_eq!(
        result[&"vendor/sub/b.rs".to_owned()],
        upstream[&"src/sub/b.rs".to_owned()],
    );
}

// ── Delegation contract ───────────────────────────────────────────────────────

/// `base_tree` is documented to equal `upstream_tree` at `entry.base`. With a
/// base recorded, the two must produce the identical tree oid.
#[test]
fn base_tree_equals_upstream_tree_at_base() {
    let (_up, _local, repo, tip) = fixture();
    let mut entry = entry("unused", vec![pat("src/**", Some("vendor/"))]);

    let direct = repo.upstream_tree(&entry, tip).expect("upstream_tree");

    entry.base = Some(tip);
    let via_base = repo.base_tree(&entry).expect("base_tree");

    assert_eq!(via_base, Some(direct));
}

// ── Error paths ───────────────────────────────────────────────────────────────

/// An unclosed `[` is not a syntax error in Git's wildmatch: the pattern
/// matches nothing. gix-glob is faithful to that — `from_bytes` accepts the
/// pattern and match time produces no results rather than an error.
#[test]
fn unclosed_bracket_matches_nothing() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry("mylib", vec![pat("src/[", None)]);

    let tree_oid = repo
        .upstream_tree(&entry, tip)
        .expect("unclosed bracket is not an error");
    assert_eq!(
        tree_oid,
        repo.find_tree(gix::hash::ObjectId::empty_tree(repo.object_hash()))
            .unwrap()
            .id(),
        "expected empty tree — pattern should match nothing"
    );
}

/// A metachar-free glob (the common single-file vendor case, e.g. `LICENSE`)
/// has a literal prefix equal to the whole glob, so the remapped path strips
/// to `""`. Rather than letting gix raise an opaque `EmptyPathComponent`,
/// `upstream_tree` reports an actionable `Error::Config` that names the file
/// and suggests adding a destination.
#[test]
fn single_file_literal_glob_is_actionable_config_error() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry("unused", vec![pat("README.md", None)]);

    match repo.upstream_tree(&entry, tip) {
        Err(Error::Config(msg)) => {
            assert!(msg.contains("README.md"), "should name the file: {msg}");
            assert!(msg.contains("destination"), "should guide the fix: {msg}");
        }
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

/// The companion of the above: giving the single-file pattern a destination
/// that includes a file name maps the file cleanly.
#[test]
fn single_file_with_named_destination_maps() {
    let (_up, _local, repo, tip) = fixture();
    let entry = entry("unused", vec![pat("README.md", Some("docs/README.md"))]);

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");
    assert_eq!(paths(&repo, tree), &["docs/README.md"]);
}

/// Remapping preserves the entry mode, not just the blob oid: an upstream
/// executable stays executable at its local path.
#[test]
fn remapping_preserves_executable_mode() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let up = upstream.path();
    git(&["init", "-b", "main"], up);
    git(&["config", "user.email", "test@example.com"], up);
    git(&["config", "user.name", "Test"], up);
    std::fs::create_dir_all(up.join("src")).unwrap();
    let script = up.join("src/run.sh");
    std::fs::write(&script, b"#!/bin/sh\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(&["add", "."], up);
    git(&["commit", "-m", "init"], up);

    let repo = make_local(local.path());
    let entry = entry(up.to_str().unwrap(), vec![pat("src/**", Some("vendor/"))]);
    let tip = repo.fetch_vendor(&entry).expect("fetch_vendor");

    let tree = repo.upstream_tree(&entry, tip).expect("upstream_tree");
    let result = repo.find_tree(tree).expect("find tree");
    let entry = result
        .traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .find(|r| r.filepath.to_str_lossy() == "vendor/run.sh")
        .expect("remapped path present");

    assert!(
        entry.mode.is_executable(),
        "executable bit must survive remap, got mode {:?}",
        entry.mode,
    );
}
