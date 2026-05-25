//! Property-based tests for invariants of `upstream_tree`.
//!
//! `upstream_tree` is a pure function of the entry's patterns and the commit:
//! it selects upstream blobs and remaps their paths, never inventing or
//! rewriting content. The invariants pinned here hold for *any* pattern set
//! over one fixed upstream tree, so the fixture (an upstream history fetched
//! into a bare local repo) is built once and shared; each case only reopens
//! the local repo and fuzzes the patterns.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use git_vendor::{
    Error, PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _,
};
use gix::bstr::ByteSlice as _;
use proptest::prelude::*;

// ── Shared fixture ────────────────────────────────────────────────────────────

struct Fixture {
    _upstream: tempfile::TempDir,
    _local: tempfile::TempDir,
    local: PathBuf,
    tip: gix::ObjectId,
}

fn git(args: &[&str], dir: &std::path::Path) {
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

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let upstream = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();

        let up = upstream.path();
        git(&["init", "-b", "main"], up);
        git(&["config", "user.email", "test@example.com"], up);
        git(&["config", "user.name", "Test"], up);
        std::fs::write(up.join("README.md"), b"readme").unwrap();
        std::fs::create_dir_all(up.join("src/sub")).unwrap();
        std::fs::write(up.join("src/a.rs"), b"a").unwrap();
        std::fs::write(up.join("src/sub/b.rs"), b"b").unwrap();
        std::fs::create_dir_all(up.join("lib")).unwrap();
        std::fs::write(up.join("lib/c.rs"), b"c").unwrap();
        git(&["add", "."], up);
        git(&["commit", "-m", "init"], up);

        git(&["init", "--bare", "-b", "main"], local.path());
        let repo = gix::open(local.path()).expect("gix open");
        let probe = VendorEntry {
            name: VendorName::new("probe").unwrap(),
            url: up.to_str().unwrap().to_owned(),
            ref_name: None,
            base: None,
            patterns: vec![],
            mode: VendorMode::Merge,
        };
        let tip = repo.fetch_vendor(&probe).expect("fetch_vendor");

        Fixture {
            local: local.path().to_path_buf(),
            tip,
            _upstream: upstream,
            _local: local,
        }
    })
}

fn blob_oids(repo: &gix::Repository, tree: gix::ObjectId) -> BTreeSet<gix::ObjectId> {
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| r.oid)
        .collect()
}

fn local_paths(repo: &gix::Repository, tree: gix::ObjectId) -> Vec<String> {
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| r.filepath.to_str_lossy().into_owned())
        .collect()
}

// ── Strategies ────────────────────────────────────────────────────────────────

fn name_strategy() -> impl Strategy<Value = VendorName> {
    "[a-z][a-z0-9-]{0,15}".prop_map(|s| VendorName::new(s).expect("strategy yields valid names"))
}

/// Globs known to be valid for `globset`, spanning empty / non-empty literal
/// prefixes and varying selectivity over the fixture layout.
fn glob_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "src/**",
        "src/*.rs",
        "src/sub/**",
        "lib/**",
        "**",
        "*.md",
        "nope/**",
    ])
    .prop_map(str::to_owned)
}

fn destination_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        prop::sample::select(vec!["vendor/", "third_party/lib/", "x/"])
            .prop_map(|d| Some(d.to_owned())),
    ]
}

fn patterns_strategy() -> impl Strategy<Value = Vec<PatternMapping>> {
    prop::collection::vec(
        (glob_strategy(), destination_strategy())
            .prop_map(|(glob, destination)| PatternMapping { glob, destination }),
        0..5,
    )
}

/// `glob_strategy` plus globs `gix_glob` rejects, so a generated pattern set
/// reliably mixes compilable and dropped entries.
fn maybe_invalid_glob_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        glob_strategy(),
        Just(String::new()),
        Just("   ".to_owned()),
        Just("# comment".to_owned()),
    ]
}

/// Raw (glob, destination) pairs; `PatternMapping` is not `Clone`, so the two
/// entries a test needs are each rebuilt from these strings.
fn raw_patterns_strategy() -> impl Strategy<Value = Vec<(String, Option<String>)>> {
    prop::collection::vec(
        (maybe_invalid_glob_strategy(), destination_strategy()),
        0..6,
    )
}

fn build(name: VendorName, raw: &[(String, Option<String>)]) -> VendorEntry {
    entry_with(
        name,
        raw.iter()
            .map(|(glob, destination)| PatternMapping {
                glob: glob.clone(),
                destination: destination.clone(),
            })
            .collect(),
    )
}

fn entry_with(name: VendorName, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name,
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns,
        mode: VendorMode::Merge,
    }
}

// ── Invariants ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// `upstream_tree` is deterministic: the same entry and commit always
    /// produce the same tree oid.
    #[test]
    fn is_deterministic(name in name_strategy(), patterns in patterns_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let entry = entry_with(name, patterns);

        let a = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        let b = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        prop_assert_eq!(a, b);
    }

    /// Selection and remapping never fabricate or rewrite content: every blob
    /// reachable in the result tree is byte-for-byte one of the upstream
    /// blobs (same oid).
    ///
    /// Note this is the only sound count-free invariant: an entry count bound
    /// would be false, since overlapping patterns with distinct destinations
    /// (e.g. `**:x/` and `**:y/`) legitimately map one upstream blob to
    /// several local paths, so the result can hold *more* entries than the
    /// upstream tree.
    #[test]
    fn preserves_upstream_blobs(name in name_strategy(), patterns in patterns_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let entry = entry_with(name, patterns);

        let upstream_tree = repo
            .find_commit(f.tip)
            .unwrap()
            .tree_id()
            .unwrap()
            .detach();
        let upstream = blob_oids(&repo, upstream_tree);

        let result = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        for oid in blob_oids(&repo, result) {
            prop_assert!(
                upstream.contains(&oid),
                "result blob {oid} is not an upstream blob (fabricated content)",
            );
        }
    }

    /// `base_tree` equals `upstream_tree` evaluated at the recorded base, for
    /// any pattern set (the documented delegation contract).
    #[test]
    fn base_tree_delegates_to_upstream_tree(
        name in name_strategy(),
        patterns in patterns_strategy(),
    ) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let mut entry = entry_with(name, patterns);

        let direct = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        entry.base = Some(f.tip);
        let via_base = repo.base_tree(&entry).expect("base_tree");

        prop_assert_eq!(via_base, Some(direct));
    }

    /// An entry with no patterns selects nothing: the result is the empty tree,
    /// independent of the vendor name.
    #[test]
    fn no_patterns_is_empty_tree(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let entry = entry_with(name, vec![]);

        let tree = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        prop_assert_eq!(tree, repo.empty_tree().id().detach());
    }

    /// Regression for the defect class my original strategy could not reach:
    /// a metachar-free glob with no destination remaps to an empty path. For
    /// *any* such glob over *any* matched file, `upstream_tree` must report
    /// `Error::Config` — never panic, never silently drop the file.
    #[test]
    fn metachar_free_glob_without_destination_is_config_error(
        name in name_strategy(),
        glob in prop::sample::select(vec![
            "README.md",
            "src/a.rs",
            "src/sub/b.rs",
            "lib/c.rs",
        ]),
    ) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let entry = entry_with(
            name,
            vec![PatternMapping { glob: glob.to_owned(), destination: None }],
        );

        match repo.upstream_tree(&entry, f.tip) {
            Err(Error::Config(_)) => {}
            other => prop_assert!(false, "expected Error::Config, got {other:?}"),
        }
    }

    /// Compiling drops globs `gix_glob` rejects, but a dropped pattern must
    /// never shift which mapping a surviving pattern uses: `upstream_tree`
    /// over the full entry equals `upstream_tree` over the entry with the
    /// uncompilable patterns already removed. (When all patterns are dropped
    /// the empty-config guard legitimately diverges, so require a survivor.)
    #[test]
    fn dropping_invalid_patterns_preserves_result(
        name in name_strategy(),
        raw in raw_patterns_strategy(),
    ) {
        let pruned: Vec<(String, Option<String>)> = raw
            .iter()
            .filter(|(g, _)| gix_glob::Pattern::from_bytes(g.as_bytes()).is_some())
            .cloned()
            .collect();
        prop_assume!(!pruned.is_empty());

        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let full = build(name.clone(), &raw);
        let pruned = build(name, &pruned);

        match (
            repo.upstream_tree(&full, f.tip),
            repo.upstream_tree(&pruned, f.tip),
        ) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(_), Err(_)) => {}
            (a, b) => prop_assert!(false, "diverged: {a:?} vs {b:?}"),
        }
    }

    /// A single pattern with a destination remaps every selected path under
    /// that destination prefix: no result path can escape it.
    #[test]
    fn single_destination_bounds_all_paths(
        name in name_strategy(),
        glob in glob_strategy(),
        dest in prop::sample::select(vec!["vendor/", "third_party/lib/", "x/"]),
    ) {
        let f = fixture();
        let repo = gix::open(&f.local).expect("gix open");
        let entry = entry_with(
            name,
            vec![PatternMapping { glob, destination: Some(dest.to_owned()) }],
        );

        let tree = repo.upstream_tree(&entry, f.tip).expect("upstream_tree");
        for path in local_paths(&repo, tree) {
            prop_assert!(path.starts_with(dest), "path {path:?} escaped {dest:?}");
        }
    }
}
