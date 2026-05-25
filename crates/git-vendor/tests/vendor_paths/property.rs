//! Property-based tests for invariants of `vendor_paths`.
//!
//! `vendor_paths` is a pure function of the vendor name and the `ours` commit:
//! it lists the commit-tree paths whose `.gitattributes` `vendor` attribute
//! resolves to the name. It never reads the upstream patterns and never
//! invents paths, and it is the path-list view of exactly what `ours_tree`
//! selects. The invariants hold for *any* name (and *any* pattern set) over
//! one fixed local history, so the fixture is built once and shared.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
use gix::bstr::{BString, ByteSlice as _};
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
/// is exactly the set `vendor_paths` must reproduce, derived without our
/// resolver.
fn oracle_paths(repo: &Path, name: &str) -> BTreeSet<String> {
    // The parser below compares `git check-attr`'s info field against `name`.
    // git emits `set`/`unset`/`unspecified` there for non-value states, so a
    // vendor name equal to one of those words would make the oracle conflate a
    // state with a value. `name_strategy` must never yield them; assert it so a
    // future strategy edit fails loudly here rather than silently miscomparing.
    debug_assert!(
        !matches!(name, "set" | "unset" | "unspecified"),
        "oracle would conflate git attribute state word {name:?} with a value",
    );

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

fn ours_blob_paths(repo: &gix::Repository, ours: gix::ObjectId) -> BTreeSet<BString> {
    let tree = repo.find_commit(ours).unwrap().tree().unwrap();
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| r.filepath)
        .collect()
}

// ── Strategies ────────────────────────────────────────────────────────────────

/// Names spanning those the fixture's `.gitattributes` yields (`libfoo`,
/// `libbar`) and ones it never does, all valid `VendorName`s.
fn name_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["libfoo", "libbar", "ghost", "lib", "libfoo-2"])
        .prop_map(str::to_owned)
}

/// Upstream patterns that meaningfully reshape `upstream_tree`; `vendor_paths`
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

fn two_distinct_names_strategy() -> impl Strategy<Value = (String, String)> {
    name_strategy().prop_flat_map(|a| {
        let a2 = a.clone();
        name_strategy()
            .prop_filter("names must differ", move |b| *b != a2)
            .prop_map(move |b| (a.clone(), b))
    })
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

    /// `vendor_paths` is deterministic: the same name and commit always
    /// produce the same list, in the same order.
    #[test]
    fn is_deterministic(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");
        let entry = entry_with(&name, vec![]);

        let a = repo.vendor_paths(&entry, f.ours).expect("vendor_paths");
        let b = repo.vendor_paths(&entry, f.ours).expect("vendor_paths");
        prop_assert_eq!(a, b);
    }

    /// `vendor_paths` is the local-side filter only: wholly independent of the
    /// entry's upstream `PatternMapping`s. Fuzzing the patterns cannot change
    /// the list.
    #[test]
    fn independent_of_upstream_patterns(
        name in name_strategy(),
        patterns in patterns_strategy(),
    ) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let baseline = repo
            .vendor_paths(&entry_with(&name, vec![]), f.ours)
            .expect("vendor_paths");
        let fuzzed = repo
            .vendor_paths(&entry_with(&name, patterns), f.ours)
            .expect("vendor_paths");
        prop_assert_eq!(fuzzed, baseline);
    }

    /// `vendor_paths` is the local filter only: like the upstream patterns,
    /// the entry's `base` and `ref_name` are upstream-side fields it must never
    /// read. Setting them cannot change the list.
    ///
    /// Forward-looking locality guard, not coverage of present behavior: the
    /// resolver's body never references `base`/`ref_name`, so this cannot fail
    /// against the implementation as written. It exists to fail loudly if a
    /// future edit makes selection depend on either upstream-side field.
    #[test]
    fn independent_of_base_and_ref_name(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let baseline = repo
            .vendor_paths(&entry_with(&name, vec![]), f.ours)
            .expect("vendor_paths");

        let mut populated = entry_with(&name, vec![]);
        populated.base = Some(f.ours);
        populated.ref_name = Some("refs/heads/some-branch".to_owned());
        let fuzzed = repo.vendor_paths(&populated, f.ours).expect("vendor_paths");

        prop_assert_eq!(fuzzed, baseline);
    }

    /// `vendor_paths` never fabricates: every listed path is an existing blob
    /// path in the `ours` commit's tree, and the list is duplicate-free.
    #[test]
    fn lists_only_existing_unique_paths(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");
        let in_tree = ours_blob_paths(&repo, f.ours);

        let paths = repo
            .vendor_paths(&entry_with(&name, vec![]), f.ours)
            .expect("vendor_paths");

        // A valid tree traversal cannot itself yield a duplicate, so the
        // uniqueness check is a cheap structural guard against a future
        // implementation that aggregates across sources; the substantive
        // assertion is the no-fabrication loop below.
        let unique: BTreeSet<&BString> = paths.iter().collect();
        prop_assert_eq!(unique.len(), paths.len(), "duplicate paths in {:?}", paths);
        for p in &paths {
            prop_assert!(in_tree.contains(p), "fabricated path {:?}", p);
        }
    }

    /// The defining invariant: `vendor_paths`'s set equals git's own attribute
    /// resolution for that name — exactly what `git check-attr` selects over
    /// the commit, no more, no less.
    #[test]
    fn matches_git_check_attr_oracle(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let paths: BTreeSet<String> = repo
            .vendor_paths(&entry_with(&name, vec![]), f.ours)
            .expect("vendor_paths")
            .into_iter()
            .map(|b| b.to_str_lossy().into_owned())
            .collect();
        prop_assert_eq!(paths, oracle_paths(&f.repo, &name));
    }

    /// The cross-method contract: `ours_tree` is the `ours` tree "restricted to
    /// `vendor_paths`". For any name, the `ours_tree` result's path set equals
    /// the `vendor_paths` set exactly.
    #[test]
    fn agrees_with_ours_tree(name in name_strategy()) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");
        let entry = entry_with(&name, vec![]);

        let listed: BTreeSet<BString> = repo
            .vendor_paths(&entry, f.ours)
            .expect("vendor_paths")
            .into_iter()
            .collect();
        let tree = repo.ours_tree(&entry, f.ours).expect("ours_tree");
        prop_assert_eq!(listed, ours_tree_paths(&repo, tree));
    }

    /// Every path component in a returned BString is non-empty and is neither
    /// `.` nor `..` — a structural guarantee of git tree paths that must hold
    /// regardless of attribute config.
    #[test]
    fn paths_have_no_unsafe_components(name in name_strategy()) {
        use gix::bstr::ByteSlice as _;

        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let paths = repo
            .vendor_paths(&entry_with(&name, vec![]), f.ours)
            .expect("vendor_paths");

        for path in &paths {
            for component in path.split_str(b"/") {
                prop_assert!(!component.is_empty(), "empty component in {path:?}");
                prop_assert!(component != b".", "`.` component in {path:?}");
                prop_assert!(component != b"..", "`..` component in {path:?}");
            }
        }
    }

    /// For any two distinct vendor names the returned path sets are always
    /// disjoint: a path attributed to one name is never attributed to another.
    #[test]
    fn two_distinct_names_have_disjoint_paths(
        (name_a, name_b) in two_distinct_names_strategy(),
    ) {
        let f = fixture();
        let repo = gix::open(&f.repo).expect("gix open");

        let a: BTreeSet<BString> = repo
            .vendor_paths(&entry_with(&name_a, vec![]), f.ours)
            .expect("vendor_paths")
            .into_iter()
            .collect();
        let b: BTreeSet<BString> = repo
            .vendor_paths(&entry_with(&name_b, vec![]), f.ours)
            .expect("vendor_paths")
            .into_iter()
            .collect();

        let shared: Vec<_> = a.intersection(&b).collect();
        prop_assert!(
            shared.is_empty(),
            "names {name_a:?} and {name_b:?} share paths: {shared:?}",
        );
    }
}
