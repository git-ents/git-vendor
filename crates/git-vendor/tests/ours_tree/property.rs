//! Property-based tests for invariants of `ours_tree`.
//!
//! `ours_tree` is a pure function of the vendor name and the `ours` commit: it
//! selects the commit-tree blobs whose `.gitattributes` `vendor` attribute
//! resolves to the name, preserving each blob's oid and path. It never reads
//! the upstream patterns and never invents content. The invariants pinned here
//! hold for *any* name (and *any* upstream pattern set) over one fixed local
//! history, so the fixture is built once and shared; each case only reopens the
//! repo and fuzzes the name and patterns.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
use gix::bstr::ByteSlice as _;
use proptest::prelude::*;

// ── Shared fixture ────────────────────────────────────────────────────────────

struct Fixture {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    ours: gix::ObjectId,
}

fn git(args: &[&str], dir: &Path) -> std::process::Output {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
    output
}

fn write(dir: &Path, rel: &str, contents: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(&["init", "-b", "main"], p);
        git(&["config", "user.email", "test@example.com"], p);
        git(&["config", "user.name", "Test"], p);
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
        // A deeper `.gitattributes` overriding the root rule for `a.rs`.
        // The oracle (`git check-attr`) computes the true resolution, so the
        // matches-oracle invariant now also pins cross-file precedence: our
        // resolver must agree with git that the nearer file wins.
        write(p, "vendor/libfoo/.gitattributes", b"a.rs vendor=libbar\n");
        git(&["add", "-A"], p);
        git(&["commit", "-m", "init"], p);
        let repo = gix::open(p).expect("gix open");
        let ours = repo.head_commit().expect("head commit").id().detach();

        Fixture {
            repo: p.to_path_buf(),
            ours,
            _dir: dir,
        }
    })
}

/// The independent oracle: git's own attribute machinery. Enumerate the tracked
/// files of `ours` and ask `git check-attr` which carry `vendor=<name>`. This
/// is exactly the set `ours_tree` must reproduce, derived without our resolver.
fn oracle_paths(repo: &Path, name: &str) -> BTreeSet<String> {
    let out = git(&["ls-tree", "-r", "--name-only", "HEAD"], repo);
    let files: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();

    let mut args = vec!["check-attr", "vendor", "--"];
    args.extend(files.iter().map(String::as_str));
    let out = git(&args, repo);
    let stdout = String::from_utf8(out.stdout).unwrap();

    let mut selected = BTreeSet::new();
    for (line, file) in stdout.lines().zip(&files) {
        // Format: `<path>: vendor: <info>`; our fixture paths are space-free.
        let info = line.rsplit(": ").next().unwrap();
        if info == name {
            selected.insert(file.clone());
        }
    }
    selected
}

fn result_paths(repo: &gix::Repository, tree: gix::ObjectId) -> BTreeSet<String> {
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

fn tree_entries(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> std::collections::BTreeMap<String, gix::ObjectId> {
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

// ── Strategies ────────────────────────────────────────────────────────────────

/// Names spanning those the fixture's `.gitattributes` yields (`libfoo`,
/// `libbar`) and ones it never does, all valid `VendorName`s.
fn name_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["libfoo", "libbar", "ghost", "lib", "libfoo-2"])
        .prop_map(str::to_owned)
}

/// Upstream patterns that meaningfully reshape `upstream_tree`; `ours_tree`
/// must be invariant under all of them.
fn patterns_strategy() -> impl Strategy<Value = Vec<PatternMapping>> {
    prop::collection::vec(
        (
            prop::sample::select(vec!["**", "vendor/**", "*.rs", "nope/**"]),
            prop_oneof![Just(None), Just(Some("dest/".to_owned()))],
        )
            .prop_map(|(glob, destination)| PatternMapping {
                glob: glob.to_owned(),
                destination,
            }),
        0..4,
    )
}

fn entry_with(name: &str, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new(name).expect("strategy yields valid names"),
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

    /// `ours_tree` is deterministic: the same name and commit always produce
    /// the same tree oid.
    #[test]
    fn is_deterministic(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");
        let entry = entry_with(&name, vec![]);

        let a = repo.ours_tree(&entry, f.ours).expect("ours_tree");
        let b = repo.ours_tree(&entry, f.ours).expect("ours_tree");
        prop_assert_eq!(a, b);
    }

    /// `ours_tree` is the local-side filter only: it is wholly independent of
    /// the entry's upstream `PatternMapping`s. For any name, fuzzing the
    /// patterns cannot change the result tree.
    #[test]
    fn independent_of_upstream_patterns(
        name in name_strategy(),
        patterns in patterns_strategy(),
    ) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let baseline = repo
            .ours_tree(&entry_with(&name, vec![]), f.ours)
            .expect("ours_tree");
        let fuzzed = repo
            .ours_tree(&entry_with(&name, patterns), f.ours)
            .expect("ours_tree");
        prop_assert_eq!(fuzzed, baseline);
    }

    /// Selection never fabricates, rewrites, or remaps: every path in the
    /// result tree exists in the `ours` commit's tree bound to the identical
    /// blob oid. (Local path space in, local path space out — unlike the
    /// upstream side, paths are not remapped.)
    #[test]
    fn preserves_ours_blobs_and_paths(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");
        let ours_full = tree_entries(
            &repo,
            repo.find_commit(f.ours).unwrap().tree_id().unwrap().detach(),
        );

        let tree = repo
            .ours_tree(&entry_with(&name, vec![]), f.ours)
            .expect("ours_tree");
        for (path, oid) in tree_entries(&repo, tree) {
            prop_assert_eq!(
                ours_full.get(&path),
                Some(&oid),
                "result path {:?} not bound to its ours blob (fabricated/remapped)",
                path,
            );
        }
    }

    /// The defining invariant: `ours_tree`'s path set equals git's own
    /// attribute resolution for that name. Whatever `git check-attr` selects
    /// over the commit, `ours_tree` selects exactly that — no more, no less.
    #[test]
    fn matches_git_check_attr_oracle(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let tree = repo
            .ours_tree(&entry_with(&name, vec![]), f.ours)
            .expect("ours_tree");
        prop_assert_eq!(
            result_paths(&repo, tree),
            oracle_paths(&f.repo, &name),
        );
    }
}
