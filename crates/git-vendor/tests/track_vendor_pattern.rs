mod common;

use git_vendor::{Vendor, VendorSource};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn with_cwd<F: FnOnce() -> T, T>(dir: &Path, f: F) -> T {
    let _guard = CWD_LOCK.lock().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = f();
    std::env::set_current_dir(prev).unwrap();
    result
}

fn test_sig() -> git2::Signature<'static> {
    git2::Signature::now("Test", "test@example.com").unwrap()
}

fn init_repo_with_gitattributes(
    gitattributes_content: &str,
) -> (git2::Repository, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    std::fs::write(tmp.path().join(".gitattributes"), gitattributes_content).unwrap();

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

fn build_tree<'a>(repo: &'a git2::Repository, files: &[(&str, &[u8])]) -> git2::Tree<'a> {
    let mut builder = repo.treebuilder(None).unwrap();

    let mut dirs: std::collections::BTreeMap<String, Vec<(String, &[u8])>> =
        std::collections::BTreeMap::new();

    for &(path, content) in files {
        let p = PathBuf::from(path);
        let components: Vec<_> = p.components().collect();
        if components.len() == 1 {
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

fn commit_tree_to_ref(
    repo: &git2::Repository,
    refname: &str,
    tree: &git2::Tree<'_>,
    message: &str,
) -> git2::Oid {
    repo.commit(Some(refname), &test_sig(), &test_sig(), message, tree, &[])
        .unwrap()
}

#[test]
fn test_track_vendor_pattern_root_glob_expands_to_per_file() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
    commit_tree_to_ref(
        &repo,
        "refs/vendor/upstream",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "upstream".into(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["*.txt:lib/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("lib/.gitattributes")).unwrap();
    assert!(
        content.contains("a.txt") && content.contains("vendor=upstream"),
        "expected a.txt vendor=upstream in:\n{content}"
    );
    assert!(
        content.contains("b.txt") && content.contains("vendor=upstream"),
        "expected b.txt vendor=upstream in:\n{content}"
    );
    assert!(
        !content.contains("*.txt"),
        "should not contain glob pattern:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_selective_glob() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[("main.rs", b"fn main(){}"), ("README.txt", b"hello")],
    );
    commit_tree_to_ref(&repo, "refs/vendor/sel", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "sel".into(),
        url: "https://example.com/sel.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["*.rs:src/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("src/.gitattributes")).unwrap();
    assert!(
        content.contains("main.rs") && content.contains("vendor=sel"),
        "expected main.rs vendor=sel in:\n{content}"
    );
    assert!(
        !content.contains("README.txt"),
        "README.txt should not be tracked:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_nested_directory() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("sub/deep.txt", b"deep")]);
    commit_tree_to_ref(
        &repo,
        "refs/vendor/nested",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "nested".into(),
        url: "https://example.com/nested.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["sub/:vendor/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("vendor/.gitattributes")).unwrap();
    assert!(
        content.contains("deep.txt"),
        "expected deep.txt in:\n{content}"
    );
    assert!(
        content.contains("vendor=nested"),
        "expected vendor=nested in:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_deep_pattern() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(&repo, &[("lib/foo.c", b"int main(){}")]);
    commit_tree_to_ref(&repo, "refs/vendor/pfx", &upstream_tree, "vendor tip");

    let vendor = VendorSource {
        name: "pfx".into(),
        url: "https://example.com/pfx.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["**/*.c:third_party/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("third_party/.gitattributes")).unwrap();
    assert!(
        content.contains("lib/foo.c") && content.contains("vendor=pfx"),
        "expected lib/foo.c vendor=pfx in:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_multiple_globs() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[
            ("main.rs", b"fn main(){}"),
            ("Cargo.toml", b"[package]"),
            ("README.txt", b"hello"),
        ],
    );
    commit_tree_to_ref(
        &repo,
        "refs/vendor/multi",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "multi".into(),
        url: "https://example.com/multi.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["*.rs:lib/".into(), "*.toml:lib/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join("lib/.gitattributes")).unwrap();
    assert!(
        content.contains("main.rs") && content.contains("vendor=multi"),
        "expected main.rs vendor=multi in:\n{content}"
    );
    assert!(
        content.contains("Cargo.toml") && content.contains("vendor=multi"),
        "expected Cargo.toml vendor=multi in:\n{content}"
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
    commit_tree_to_ref(
        &repo,
        "refs/vendor/nomatch",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "nomatch".into(),
        url: "https://example.com/nomatch.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["*.rs:src/".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        !content.contains("vendor="),
        "no vendor attribute expected:\n{content}"
    );
}

#[test]
fn test_track_vendor_pattern_expands_to_per_file() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let upstream_tree = build_tree(
        &repo,
        &[
            ("a.txt", b"aaa"),
            ("sub/b.txt", b"bbb"),
            ("README.md", b"# hi"),
        ],
    );
    commit_tree_to_ref(
        &repo,
        "refs/vendor/expand",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "expand".into(),
        url: "https://example.com/expand.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["**/*.txt".into()],
    };

    with_cwd(tmp.path(), || {
        repo.track_vendor_pattern(&vendor).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        content.contains("a.txt vendor=expand"),
        "expected a.txt entry in:\n{content}"
    );
    assert!(
        content.contains("sub/b.txt vendor=expand"),
        "expected sub/b.txt entry in:\n{content}"
    );
    assert!(
        !content.contains("*.txt"),
        "should not contain glob pattern:\n{content}"
    );
    assert!(
        !content.contains("README.md"),
        "README.md should not be tracked:\n{content}"
    );
}
