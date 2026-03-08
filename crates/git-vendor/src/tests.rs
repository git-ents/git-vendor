//! Unit tests for `git-vendor`.

use git2::Config;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::{NamedTempFile, TempPath};

use super::{Vendor, VendorSource};

/// Global lock so that tests that change the process-wide CWD don't race.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` with the working directory temporarily set to `dir`.
/// Acquires `CWD_LOCK` so concurrent tests don't interfere.
fn with_cwd<F: FnOnce() -> T, T>(dir: &Path, f: F) -> T {
    let _guard = CWD_LOCK.lock().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = f();
    std::env::set_current_dir(prev).unwrap();
    result
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write an ini-style git config into a temp file and return both so the
/// caller can keep the `NamedTempFile` alive for as long as the `Config` is
/// needed (dropping the file closes/deletes it, which would invalidate the
/// config handle on some platforms).
fn config_from_str(s: &str) -> (TempPath, Config) {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(s.as_bytes()).unwrap();
    // Close the open file handle before handing the path to libgit2.
    // On Windows, libgit2 writes config changes via a lockfile rename, which
    // fails with "Access is denied" when the underlying file is still open.
    let path = tmp.into_temp_path();
    let cfg = Config::open(&path).unwrap();
    (path, cfg)
}

/// Return a writable `Config` backed by an empty temp file.
fn empty_config() -> (TempPath, Config) {
    config_from_str("")
}

// ---------------------------------------------------------------------------
// VendorSource::head_ref
// ---------------------------------------------------------------------------

#[test]
fn test_head_ref_simple() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };
    assert_eq!(vs.head_ref(), "refs/vendor/foo");
}

#[test]
fn test_head_ref_with_hyphens_and_underscores() {
    let vs = VendorSource {
        name: "my-cool_lib".into(),
        url: "https://example.com/lib.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };
    assert_eq!(vs.head_ref(), "refs/vendor/my-cool_lib");
}

// ---------------------------------------------------------------------------
// VendorSource::tracking_branch
// ---------------------------------------------------------------------------

#[test]
fn test_tracking_branch_defaults_to_head() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };
    assert_eq!(vs.tracking_branch(), "HEAD");
}

#[test]
fn test_tracking_branch_uses_explicit_branch() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: Some("main".into()),
        base: None,
        patterns: vec![],
    };
    assert_eq!(vs.tracking_branch(), "main");
}

#[test]
fn test_tracking_branch_arbitrary_name() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: Some("release/v2".into()),
        base: None,
        patterns: vec![],
    };
    assert_eq!(vs.tracking_branch(), "release/v2");
}

// ---------------------------------------------------------------------------
// VendorSource::to_config
// ---------------------------------------------------------------------------

#[test]
fn test_to_config_url_only() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.foo.url").unwrap(),
        "https://example.com/foo.git"
    );
    // Optional fields must be absent.
    assert!(cfg.get_string("vendor.foo.branch").is_err());
    assert!(cfg.get_string("vendor.foo.base").is_err());
}

#[test]
fn test_to_config_with_branch() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "bar".into(),
        url: "https://example.com/bar.git".into(),
        branch: Some("develop".into()),
        base: None,
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.bar.url").unwrap(),
        "https://example.com/bar.git"
    );
    assert_eq!(cfg.get_string("vendor.bar.branch").unwrap(), "develop");
    assert!(cfg.get_string("vendor.bar.base").is_err());
}

#[test]
fn test_to_config_with_base() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "baz".into(),
        url: "https://example.com/baz.git".into(),
        branch: None,
        base: Some("cafebabe".into()),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.baz.url").unwrap(),
        "https://example.com/baz.git"
    );
    assert!(cfg.get_string("vendor.baz.branch").is_err());
    assert_eq!(cfg.get_string("vendor.baz.base").unwrap(), "cafebabe");
}

#[test]
fn test_to_config_all_fields() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "full".into(),
        url: "https://example.com/full.git".into(),
        branch: Some("stable".into()),
        base: Some("deadbeef".into()),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.full.url").unwrap(),
        "https://example.com/full.git"
    );
    assert_eq!(cfg.get_string("vendor.full.branch").unwrap(), "stable");
    assert_eq!(cfg.get_string("vendor.full.base").unwrap(), "deadbeef");
}

// ---------------------------------------------------------------------------
// VendorSource::from_config
// ---------------------------------------------------------------------------

#[test]
fn test_from_config_returns_none_for_unknown_vendor() {
    let (_tmp, cfg) = config_from_str("");
    let result = VendorSource::from_config(&cfg, "ghost").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_from_config_minimal() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "solo"]
    url = https://example.com/solo.git
"#,
    );
    let vs = VendorSource::from_config(&cfg, "solo").unwrap().unwrap();

    assert_eq!(vs.name, "solo");
    assert_eq!(vs.url, "https://example.com/solo.git");
    assert!(vs.branch.is_none());
    assert!(vs.base.is_none());
}

#[test]
fn test_from_config_with_branch() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "alpha"]
    url = https://example.com/alpha.git
    branch = next
"#,
    );
    let vs = VendorSource::from_config(&cfg, "alpha").unwrap().unwrap();

    assert_eq!(vs.name, "alpha");
    assert_eq!(vs.url, "https://example.com/alpha.git");
    assert_eq!(vs.branch.as_deref(), Some("next"));
    assert!(vs.base.is_none());
}

#[test]
fn test_from_config_with_base() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "beta"]
    url = https://example.com/beta.git
    base = 1a2b3c4d
"#,
    );
    let vs = VendorSource::from_config(&cfg, "beta").unwrap().unwrap();

    assert_eq!(vs.name, "beta");
    assert_eq!(vs.url, "https://example.com/beta.git");
    assert!(vs.branch.is_none());
    assert_eq!(vs.base.as_deref(), Some("1a2b3c4d"));
}

#[test]
fn test_from_config_all_fields() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "gamma"]
    url = https://example.com/gamma.git
    branch = release
    base = 0000ffff
"#,
    );
    let vs = VendorSource::from_config(&cfg, "gamma").unwrap().unwrap();

    assert_eq!(vs.name, "gamma");
    assert_eq!(vs.url, "https://example.com/gamma.git");
    assert_eq!(vs.branch.as_deref(), Some("release"));
    assert_eq!(vs.base.as_deref(), Some("0000ffff"));
}

#[test]
fn test_from_config_does_not_match_other_vendors() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "real"]
    url = https://example.com/real.git
"#,
    );
    // A different name must not be confused with "real".
    let result = VendorSource::from_config(&cfg, "real2").unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// VendorSource to_config / from_config roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_config_roundtrip_full() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "roundtrip".into(),
        url: "https://example.com/roundtrip.git".into(),
        branch: Some("main".into()),
        base: Some("abc123def456".into()),
        patterns: vec![".config/".into(), ".github/".into()],
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "roundtrip")
        .unwrap()
        .unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert_eq!(restored.branch, original.branch);
    assert_eq!(restored.base, original.base);
    assert_eq!(restored.patterns, original.patterns);
}

#[test]
fn test_config_roundtrip_optional_fields_absent() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "minimal".into(),
        url: "https://example.com/minimal.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "minimal").unwrap().unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert!(restored.branch.is_none());
    assert!(restored.base.is_none());
    assert!(restored.patterns.is_empty());
}

#[test]
fn test_config_roundtrip_patterns_update() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "pat".into(),
        url: "https://example.com/pat.git".into(),
        branch: None,
        base: None,
        patterns: vec![".config/".into(), ".github/".into()],
    };
    original.to_config(&mut cfg).unwrap();

    // Update patterns: remove one, add another.
    let updated = VendorSource {
        patterns: vec![".github/".into(), "src/".into()],
        ..original.clone()
    };
    updated.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "pat").unwrap().unwrap();
    assert_eq!(
        restored.patterns,
        vec![".github/".to_string(), "src/".to_string()]
    );
}

// ---------------------------------------------------------------------------
// vendors_from_config
// ---------------------------------------------------------------------------

#[test]
fn test_gitvendors_list() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "foo"]
    url = https://example.com/foo.git
    branch = main

[vendor "bar"]
    url = https://example.com/bar.git
    base = deadbeef123
"#,
    );

    let mut vendors = super::vendors_from_config(&cfg).unwrap();
    vendors.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(vendors.len(), 2);

    assert_eq!(vendors[0].name, "bar");
    assert_eq!(vendors[0].url, "https://example.com/bar.git");
    assert_eq!(vendors[0].branch, None);
    assert_eq!(vendors[0].base.as_deref(), Some("deadbeef123"));

    assert_eq!(vendors[1].name, "foo");
    assert_eq!(vendors[1].url, "https://example.com/foo.git");
    assert_eq!(vendors[1].branch.as_deref(), Some("main"));
    assert_eq!(vendors[1].base, None);
}

#[test]
fn test_vendors_from_config_empty() {
    let (_tmp, cfg) = config_from_str("");
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert!(vendors.is_empty());
}

#[test]
fn test_vendors_from_config_single_entry() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "only"]
    url = https://example.com/only.git
"#,
    );
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert_eq!(vendors.len(), 1);
    assert_eq!(vendors[0].name, "only");
    assert_eq!(vendors[0].url, "https://example.com/only.git");
}

#[test]
fn test_vendors_from_config_ignores_non_vendor_sections() {
    let (_tmp, cfg) = config_from_str(
        r#"
[core]
    repositoryformatversion = 0

[vendor "legit"]
    url = https://example.com/legit.git
"#,
    );
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert_eq!(vendors.len(), 1);
    assert_eq!(vendors[0].name, "legit");
}

// ---------------------------------------------------------------------------
// bail_if_bare
// ---------------------------------------------------------------------------

#[test]
fn test_bail_if_bare_accepts_non_bare_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    assert!(super::bail_if_bare(&repo).is_ok());
}

#[test]
fn test_bail_if_bare_rejects_bare_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init_bare(tmp.path()).unwrap();
    let err = super::bail_if_bare(&repo).unwrap_err();
    assert!(
        err.message()
            .contains("bare repositories are not supported"),
        "unexpected error message: {}",
        err.message()
    );
}

// ---------------------------------------------------------------------------
// Integration-test helpers
// ---------------------------------------------------------------------------

/// Signature used for all test commits.
fn test_sig() -> git2::Signature<'static> {
    git2::Signature::now("Test", "test@example.com").unwrap()
}

/// Create a non-bare repo in a tempdir, with an initial commit containing
/// a `.gitattributes` file (possibly empty) so that `set_attr` / `get_attr`
/// work correctly.
///
/// Returns `(repo, tempdir)` – keep `tempdir` alive for the repo's lifetime.
fn init_repo_with_gitattributes(
    gitattributes_content: &str,
) -> (git2::Repository, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    // Write .gitattributes to working tree
    std::fs::write(tmp.path().join(".gitattributes"), gitattributes_content).unwrap();

    // Stage and commit
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".gitattributes")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();

        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "initial commit",
            &tree,
            &[],
        )
        .unwrap();
    }

    (repo, tmp)
}

/// Build a tree in `repo` from a list of `(path, content)` pairs.
/// Nested directories are created automatically.
fn build_tree<'a>(repo: &'a git2::Repository, files: &[(&str, &[u8])]) -> git2::Tree<'a> {
    let mut builder = repo.treebuilder(None).unwrap();

    // Group files by top-level component so we can build subtrees.
    let mut dirs: std::collections::BTreeMap<String, Vec<(String, &[u8])>> =
        std::collections::BTreeMap::new();

    for &(path, content) in files {
        let p = PathBuf::from(path);
        let components: Vec<_> = p.components().collect();
        if components.len() == 1 {
            // root-level file
            let blob = repo.blob(content).unwrap();
            builder
                .insert(components[0].as_os_str().to_str().unwrap(), blob, 0o100644)
                .unwrap();
        } else {
            let dir = components[0].as_os_str().to_str().unwrap().to_string();
            let rest: PathBuf = components[1..].iter().collect();
            dirs.entry(dir)
                .or_default()
                .push((rest.to_string_lossy().into_owned(), content));
        }
    }

    for (dir, entries) in &dirs {
        let sub_files: Vec<(&str, &[u8])> = entries.iter().map(|(p, c)| (p.as_str(), *c)).collect();
        let subtree = build_tree(repo, &sub_files);
        builder
            .insert(dir.as_str(), subtree.id(), 0o040000)
            .unwrap();
    }

    let oid = builder.write().unwrap();
    repo.find_tree(oid).unwrap()
}

/// Create a commit from a tree and attach it to the given refname.
fn commit_tree_to_ref(
    repo: &git2::Repository,
    refname: &str,
    tree: &git2::Tree<'_>,
    message: &str,
) -> git2::Oid {
    let oid = repo
        .commit(Some(refname), &test_sig(), &test_sig(), message, tree, &[])
        .unwrap();
    oid
}

// ---------------------------------------------------------------------------
// track_vendor_pattern – integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_track_vendor_pattern_root_glob_expands_to_per_file() {
    // Upstream tree has two root-level files.
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
    commit_tree_to_ref(&repo, "refs/vendor/upstream", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "upstream".into(),
        url: "https://example.com/upstream.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["*.txt"], Path::new("lib"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("lib/.gitattributes")).unwrap();
    assert!(
        content.contains("lib/a.txt") && content.contains("vendor=upstream"),
        "expected lib/a.txt vendor=upstream in:\n{content}"
    );
    assert!(
        content.contains("lib/b.txt") && content.contains("vendor=upstream"),
        "expected lib/b.txt vendor=upstream in:\n{content}"
    );
    // No glob pattern.
    assert!(
        !content.contains("*.txt"),
        "should not contain glob pattern:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_selective_glob() {
    // Upstream tree has a .rs and a .txt file – only .rs should be tracked.
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[("main.rs", b"fn main(){}"), ("README.txt", b"hello")],
    );
    commit_tree_to_ref(&repo, "refs/vendor/sel", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "sel".into(),
        url: "https://example.com/sel.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["*.rs"], Path::new("src"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("src/.gitattributes")).unwrap();
    assert!(
        content.contains("src/main.rs") && content.contains("vendor=sel"),
        "expected src/main.rs vendor=sel in:\n{content}"
    );
    // README.txt must NOT appear
    assert!(
        !content.contains("README.txt"),
        "README.txt should not be tracked:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_nested_directory() {
    // Upstream tree: sub/deep.txt
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("sub/deep.txt", b"deep")]);
    commit_tree_to_ref(&repo, "refs/vendor/nested", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "nested".into(),
        url: "https://example.com/nested.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    // Use `sub/` glob — should expand to per-file entry for sub/deep.txt
    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["sub/"], Path::new("vendor"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("vendor/.gitattributes")).unwrap();
    assert!(
        content.contains("vendor/sub/deep.txt"),
        "expected vendor/sub/deep.txt in:\n{content}"
    );
    assert!(
        content.contains("vendor=nested"),
        "expected vendor=nested in:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_deep_pattern() {
    // Upstream tree: lib/foo.c
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("lib/foo.c", b"int main(){}")]);
    commit_tree_to_ref(&repo, "refs/vendor/pfx", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "pfx".into(),
        url: "https://example.com/pfx.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["**/*.c"], Path::new("third_party"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("third_party/.gitattributes")).unwrap();
    assert!(
        content.contains("third_party/lib/foo.c") && content.contains("vendor=pfx"),
        "expected third_party/lib/foo.c vendor=pfx in:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_multiple_globs() {
    // Upstream tree has .rs, .toml, and .txt files – only .rs and .toml should be tracked.
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[
            ("main.rs", b"fn main(){}"),
            ("Cargo.toml", b"[package]"),
            ("README.txt", b"hello"),
        ],
    );
    commit_tree_to_ref(&repo, "refs/vendor/multi", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "multi".into(),
        url: "https://example.com/multi.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["*.rs", "*.toml"], Path::new("lib"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("lib/.gitattributes")).unwrap();
    assert!(
        content.contains("lib/main.rs") && content.contains("vendor=multi"),
        "expected lib/main.rs vendor=multi in:\n{content}"
    );
    assert!(
        content.contains("lib/Cargo.toml") && content.contains("vendor=multi"),
        "expected lib/Cargo.toml vendor=multi in:\n{content}"
    );
    assert!(
        !content.contains("README.txt"),
        "README.txt should not be tracked:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_no_match_leaves_gitattributes_unchanged() {
    let (repo, tmp) = init_repo_with_gitattributes("# existing\n");
    let upstream_tree = build_tree(&repo, &[("data.bin", b"\x00\x01")]);
    commit_tree_to_ref(&repo, "refs/vendor/nomatch", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "nomatch".into(),
        url: "https://example.com/nomatch.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["*.rs"], Path::new("src"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    // Only the original comment should remain (plus a trailing newline).
    assert!(
        !content.contains("vendor="),
        "no vendor attribute expected:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_expands_to_per_file() {
    // Each matched file gets its own gitattributes line.
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[
            ("a.txt", b"aaa"),
            ("sub/b.txt", b"bbb"),
            ("README.md", b"# hi"),
        ],
    );
    commit_tree_to_ref(&repo, "refs/vendor/expand", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "expand".into(),
        url: "https://example.com/expand.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor, &["**/*.txt"], Path::new("."))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    // Per-file entries, not a glob pattern.
    assert!(
        content.contains("a.txt vendor=expand"),
        "expected a.txt entry in:\n{content}"
    );
    assert!(
        content.contains("sub/b.txt vendor=expand"),
        "expected sub/b.txt entry in:\n{content}"
    );
    // No glob pattern.
    assert!(
        !content.contains("*.txt"),
        "should not contain glob pattern:\n{content}"
    );
    // Unmatched file should be absent.
    assert!(
        !content.contains("README.md"),
        "README.md should not be tracked:\n{content}"
    );
}

// ---------------------------------------------------------------------------
// merge_vendor – integration tests
// ---------------------------------------------------------------------------

/// Helper: commit working-tree changes (stages everything) and return the
/// commit OID.  `parents` are the parent commits.
fn commit_workdir(
    repo: &git2::Repository,
    message: &str,
    parents: &[&git2::Commit<'_>],
) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    repo.commit(
        Some("HEAD"),
        &test_sig(),
        &test_sig(),
        message,
        &tree,
        parents,
    )
    .unwrap()
}

/// Set up a local repo whose HEAD contains:
///   - `.gitattributes` with `vendor` attrs for given files
///   - the vendored files themselves
///
/// Also creates `refs/vendor/<name>` pointing at a commit with the upstream
/// tree.
///
/// Returns `(repo, tempdir, vendor_source)`.
fn setup_merge_scenario(
    vendor_name: &str,
    // (local_path, content) – the local file and its content.
    // Local paths must match the corresponding upstream paths.
    local_files: &[(&str, &[u8])],
    // (remote_path, content) – full paths in the upstream tree.
    remote_files: &[(&str, &[u8])],
    // Glob patterns for the vendor.
    patterns: &[&str],
) -> (git2::Repository, tempfile::TempDir, VendorSource) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    // Build .gitattributes content
    let mut attrs = String::new();
    for &(local_path, _) in local_files {
        attrs.push_str(&format!("{local_path} vendor={vendor_name}\n"));
    }

    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();

    // Write local vendored files to the working tree
    for &(local_path, content) in local_files {
        let full = tmp.path().join(local_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }

    // Initial commit
    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "initial",
            &tree,
            &[],
        )
        .unwrap();
    }

    // Build the upstream (vendor) tree and create a ref for it.
    {
        let upstream_tree = build_tree(&repo, remote_files);
        commit_tree_to_ref(
            &repo,
            &format!("refs/vendor/{vendor_name}"),
            &upstream_tree,
            "upstream tip",
        );
    }

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        branch: None,
        base: None,
        patterns: patterns.iter().map(|s| s.to_string()).collect(),
    };

    (repo, tmp, vendor)
}

#[test]
fn test_merge_vendor_no_base_identical_content() {
    // Local and upstream have identical content → merge index has no conflicts.
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "lib",
        &[("src/hello.c", b"int main(){}")],
        &[("src/hello.c", b"int main(){}")],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "identical content should produce a clean merge"
    );
}

#[test]
fn test_merge_vendor_no_base_upstream_changed() {
    // Local has original content; upstream has new content.
    // Without a base, ours_filtered is used as the base, so this should be a
    // fast-forward-style merge that picks up the upstream change cleanly.
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "ext",
        &[("include/util.h", b"// v1")],
        &[("include/util.h", b"// v2")],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None, None).unwrap();
    // The merge should succeed without conflicts because the base equals ours.
    assert!(
        !index.has_conflicts(),
        "upstream-only change should merge cleanly when no base exists"
    );
}

#[test]
fn test_merge_vendor_with_base_clean_merge() {
    // Set up a scenario where a base commit exists and both sides diverge
    // in non-overlapping ways.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let vendor_name = "clean";

    // .gitattributes
    let attrs = "a.txt vendor=clean\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();

    // Original local file
    std::fs::write(tmp.path().join("a.txt"), "line1\nline2\nline3\n").unwrap();

    // Initial commit (this is our "base" state)
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let base_oid = repo
        .commit(Some("HEAD"), &test_sig(), &test_sig(), "base", &tree, &[])
        .unwrap();

    // Now update local: change line1 → lineA (ours diverges)
    std::fs::write(tmp.path().join("a.txt"), "lineA\nline2\nline3\n").unwrap();
    let base_commit = repo.find_commit(base_oid).unwrap();
    commit_workdir(&repo, "local change", &[&base_commit]);

    // Build upstream tree: change line3 → lineC (theirs diverges)
    let upstream_tree = build_tree(&repo, &[("a.txt", b"line1\nline2\nlineC\n")]);
    commit_tree_to_ref(
        &repo,
        &format!("refs/vendor/{vendor_name}"),
        &upstream_tree,
        "upstream change",
    );

    // The base tree is the filtered tree at the base commit.
    // We build it identically to how the code will see it.
    let base_tree = build_tree(&repo, &[("a.txt", b"line1\nline2\nline3\n")]);
    let base_tree_commit = repo
        .commit(
            None,
            &test_sig(),
            &test_sig(),
            "base tree commit",
            &base_tree,
            &[],
        )
        .unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        branch: None,
        base: Some(base_tree_commit.to_string()),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None, None).unwrap();
    assert!(
        !idx.has_conflicts(),
        "non-overlapping changes should merge cleanly with a base"
    );
}

#[test]
fn test_merge_vendor_conflict() {
    // Both sides modify the same region → conflict expected.
    //
    // Use a root-level vendored file so that base / ours / theirs all share
    // the same path, avoiding rename-detection ambiguity.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let vendor_name = "conflict";

    let attrs = "f.txt vendor=conflict\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();

    std::fs::write(tmp.path().join("f.txt"), "original\n").unwrap();

    // Base commit
    let base_oid = {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &test_sig(), &test_sig(), "base", &tree, &[])
            .unwrap()
    };

    // Local changes the content
    std::fs::write(tmp.path().join("f.txt"), "local change\n").unwrap();
    let base_commit = repo.find_commit(base_oid).unwrap();
    commit_workdir(&repo, "local edit", &[&base_commit]);

    // Upstream also changes the content differently
    {
        let upstream_tree = build_tree(&repo, &[("f.txt", b"upstream change\n")]);
        commit_tree_to_ref(
            &repo,
            &format!("refs/vendor/{vendor_name}"),
            &upstream_tree,
            "upstream edit",
        );
    }

    // Base tree uses the same root-level path, matching ours_filtered
    let base_tree_commit = {
        let base_tree = build_tree(&repo, &[("f.txt", b"original\n")]);
        repo.commit(None, &test_sig(), &test_sig(), "base tree", &base_tree, &[])
            .unwrap()
    };

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        branch: None,
        base: Some(base_tree_commit.to_string()),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None, None).unwrap();
    assert!(
        idx.has_conflicts(),
        "both sides modifying the same region should produce a conflict"
    );
}

#[test]
fn test_merge_vendor_multiple_files() {
    // Merge with two vendored files; upstream updates only one.
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "multi",
        &[("one.txt", b"one"), ("two.txt", b"two")],
        &[
            ("one.txt", b"one"),     // unchanged
            ("two.txt", b"two-new"), // changed
        ],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None, None).unwrap();
    // Without a base the filtered-ours is used as base, so this should be
    // a clean merge that picks up the upstream change.
    assert!(
        !index.has_conflicts(),
        "partial upstream update should merge cleanly without a base"
    );
}

#[test]
fn test_merge_vendor_filters_unrelated_upstream_files() {
    // Upstream tree contains extra files beyond what the pattern selects.
    // Only files matching the pattern should appear in the merge result.
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "filter",
        &[("src/core.rs", b"fn core(){}")],
        &[
            ("src/core.rs", b"fn core(){}"),
            ("src/extra.rs", b"fn extra(){}"), // not matched by pattern
            ("README.md", b"# hello"),         // not matched by pattern
        ],
        &["**/core.rs"],
    );

    let index = repo.merge_vendor(&vendor, None, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "unmatched upstream files should be filtered out, leaving a clean merge"
    );

    // Verify that the merge index only contains the pattern-matched file
    // (plus the gitattributes entry from ours).
    let entries: Vec<_> = index.iter().collect();
    let paths: Vec<String> = entries
        .iter()
        .map(|e| String::from_utf8_lossy(&e.path).to_string())
        .collect();
    assert!(
        !paths.contains(&"src/extra.rs".to_string()),
        "extra.rs should not be in merge index, got: {paths:?}"
    );
    assert!(
        !paths.contains(&"README.md".to_string()),
        "README.md should not be in merge index, got: {paths:?}"
    );
}

#[test]
fn test_merge_vendor_picks_up_new_upstream_file() {
    // Upstream adds a new file that matches the vendor's patterns.
    // merge_vendor should include it even though no local gitattributes
    // entry exists for it yet.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let vendor_name = "newfile";

    // Local has one vendored file.
    let attrs = "a.txt vendor=newfile\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "original\n").unwrap();

    // Initial commit.
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let _base_oid = repo
        .commit(Some("HEAD"), &test_sig(), &test_sig(), "base", &tree, &[])
        .unwrap();

    // Upstream adds b.txt alongside a.txt.
    let upstream_tree = build_tree(&repo, &[("a.txt", b"original\n"), ("b.txt", b"new file\n")]);
    commit_tree_to_ref(
        &repo,
        &format!("refs/vendor/{vendor_name}"),
        &upstream_tree,
        "upstream adds b.txt",
    );

    let base_tree = build_tree(&repo, &[("a.txt", b"original\n")]);
    let base_tree_commit = repo
        .commit(
            None,
            &test_sig(),
            &test_sig(),
            "base tree commit",
            &base_tree,
            &[],
        )
        .unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        branch: None,
        base: Some(base_tree_commit.to_string()),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None, None).unwrap();
    assert!(
        !idx.has_conflicts(),
        "adding a new file should merge cleanly"
    );

    let paths: Vec<String> = idx
        .iter()
        .map(|e| String::from_utf8_lossy(&e.path).to_string())
        .collect();
    assert!(
        paths.contains(&"b.txt".to_string()),
        "new upstream file b.txt should be in merge index, got: {paths:?}"
    );
}

// ---------------------------------------------------------------------------
// Forward-slash correctness (Windows regression tests)
// ---------------------------------------------------------------------------

#[test]
fn test_refresh_vendor_attrs_uses_forward_slashes() {
    // refresh_vendor_attrs writes per-file entries into .gitattributes.
    // The paths must use forward slashes even on Windows.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    // Write initial .gitattributes and a nested vendored file, then commit.
    std::fs::write(tmp.path().join(".gitattributes"), "").unwrap();
    std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("sub/file.txt"), "hello").unwrap();

    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "initial",
            &tree,
            &[],
        )
        .unwrap();
    }

    // Build a merged index containing a nested path.
    let mut merged_index = git2::Index::new().unwrap();
    let blob_oid = repo.blob(b"hello").unwrap();
    merged_index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: 5,
            id: blob_oid,
            flags: 0,
            flags_extended: 0,
            path: b"sub/file.txt".to_vec(),
        })
        .unwrap();

    let vendor = VendorSource {
        name: "fwdslash".into(),
        url: "https://example.com/fwdslash.git".into(),
        branch: None,
        base: None,
        patterns: vec!["**".into()],
    };

    with_cwd(tmp.path(), || {
        repo.refresh_vendor_attrs(&vendor, &merged_index, Path::new("lib"))
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("lib/.gitattributes")).unwrap();
    assert!(
        content.contains("lib/sub/file.txt"),
        "expected forward-slash path lib/sub/file.txt in:\n{content}"
    );
    assert!(
        !content.contains('\\'),
        "gitattributes must not contain backslashes:\n{content}"
    );
}

#[test]
fn test_add_vendor_nested_paths_match_correctly() {
    // add_vendor must match nested upstream paths against HEAD even when
    // Path separators differ (Windows backslash vs git forward-slash).
    let (repo, tmp) = init_repo_with_gitattributes("");

    // Add a nested file to HEAD so add_vendor can detect the overlap.
    std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("sub/overlap.c"), "// local v1").unwrap();
    {
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "add local file",
            &tree,
            &[&head],
        )
        .unwrap();
    }

    // Build upstream tree with a nested file at the same path.
    let upstream_tree = build_tree(&repo, &[("sub/overlap.c", b"// upstream v1")]);
    commit_tree_to_ref(&repo, "refs/vendor/nested", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "nested".into(),
        url: "https://example.com/nested.git".into(),
        branch: None,
        base: None,
        patterns: vec![],
    };

    with_cwd(tmp.path(), || {
        let index = repo
            .add_vendor(&vendor, &["sub/"], Path::new("."), None)
            .unwrap();

        // The merge should detect both sides and produce a result containing
        // the nested path (possibly conflicted, but present).
        let paths: Vec<String> = index
            .iter()
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .collect();
        assert!(
            paths.contains(&"sub/overlap.c".to_string()),
            "nested file sub/overlap.c should be in merge index, got: {paths:?}"
        );
    });
}
