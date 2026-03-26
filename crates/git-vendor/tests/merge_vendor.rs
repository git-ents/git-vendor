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

fn _init_repo_with_gitattributes(
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

fn write_gitvendors(tmp: &Path, vendor: &VendorSource) {
    let path = tmp.join(".gitvendors");
    if !path.exists() {
        std::fs::write(&path, "").unwrap();
    }
    let mut cfg = git2::Config::open(&path).unwrap();
    vendor.to_config(&mut cfg).unwrap();
}

fn setup_merge_scenario(
    vendor_name: &str,
    local_files: &[(&str, &[u8])],
    remote_files: &[(&str, &[u8])],
    patterns: &[&str],
) -> (git2::Repository, tempfile::TempDir, VendorSource) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let mut attrs = String::new();
    for &(local_path, _) in local_files {
        attrs.push_str(&format!("{local_path} vendor={vendor_name}\n"));
    }

    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();

    for &(local_path, content) in local_files {
        let full = tmp.path().join(local_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }

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
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: patterns.iter().map(|s| s.to_string()).collect(),
    };

    (repo, tmp, vendor)
}

#[test]
fn test_merge_vendor_no_base_identical_content() {
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "lib",
        &[("src/hello.c", b"int main(){}")],
        &[("src/hello.c", b"int main(){}")],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "identical content should produce a clean merge"
    );
}

#[test]
fn test_merge_vendor_no_base_upstream_changed() {
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "ext",
        &[("include/util.h", b"// v1")],
        &[("include/util.h", b"// v2")],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "upstream-only change should merge cleanly when no base exists"
    );
}

#[test]
fn test_merge_vendor_with_base_clean_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "clean";

    let attrs = "a.txt vendor=clean\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "line1\nline2\nline3\n").unwrap();

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

    std::fs::write(tmp.path().join("a.txt"), "lineA\nline2\nline3\n").unwrap();
    let base_commit = repo.find_commit(base_oid).unwrap();
    commit_workdir(&repo, "local change", &[&base_commit]);

    let upstream_tree = build_tree(&repo, &[("a.txt", b"line1\nline2\nlineC\n")]);
    commit_tree_to_ref(
        &repo,
        &format!("refs/vendor/{vendor_name}"),
        &upstream_tree,
        "upstream change",
    );

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
        ref_name: None,
        base: Some(base_tree_commit.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !idx.has_conflicts(),
        "non-overlapping changes should merge cleanly with a base"
    );
}

#[test]
fn test_merge_vendor_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "conflict";

    let attrs = "f.txt vendor=conflict\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();
    std::fs::write(tmp.path().join("f.txt"), "original\n").unwrap();

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

    std::fs::write(tmp.path().join("f.txt"), "local change\n").unwrap();
    let base_commit = repo.find_commit(base_oid).unwrap();
    commit_workdir(&repo, "local edit", &[&base_commit]);

    {
        let upstream_tree = build_tree(&repo, &[("f.txt", b"upstream change\n")]);
        commit_tree_to_ref(
            &repo,
            &format!("refs/vendor/{vendor_name}"),
            &upstream_tree,
            "upstream edit",
        );
    }

    let base_tree_commit = {
        let base_tree = build_tree(&repo, &[("f.txt", b"original\n")]);
        repo.commit(None, &test_sig(), &test_sig(), "base tree", &base_tree, &[])
            .unwrap()
    };

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(base_tree_commit.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        idx.has_conflicts(),
        "both sides modifying the same region should produce a conflict"
    );
}

#[test]
fn test_merge_vendor_multiple_files() {
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "multi",
        &[("one.txt", b"one"), ("two.txt", b"two")],
        &[("one.txt", b"one"), ("two.txt", b"two-new")],
        &["**"],
    );

    let index = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "partial upstream update should merge cleanly without a base"
    );
}

#[test]
fn test_merge_vendor_filters_unrelated_upstream_files() {
    let (repo, _tmp, vendor) = setup_merge_scenario(
        "filter",
        &[("src/core.rs", b"fn core(){}")],
        &[
            ("src/core.rs", b"fn core(){}"),
            ("src/extra.rs", b"fn extra(){}"),
            ("README.md", b"# hello"),
        ],
        &["**/core.rs"],
    );

    let index = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !index.has_conflicts(),
        "unmatched upstream files should be filtered out, leaving a clean merge"
    );

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
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "newfile";

    let attrs = "a.txt vendor=newfile\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "original\n").unwrap();

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
        ref_name: None,
        base: Some(base_tree_commit.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };

    let idx = repo.merge_vendor(&vendor, None).unwrap();
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

#[test]
fn test_merge_vendor_new_file_at_mapped_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "mapped";

    let attrs = "ext/a.rs vendor=mapped\n";
    std::fs::write(tmp.path().join(".gitattributes"), attrs).unwrap();
    std::fs::create_dir_all(tmp.path().join("ext")).unwrap();
    std::fs::write(tmp.path().join("ext/a.rs"), "fn a(){}\n").unwrap();

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

    let upstream_tree = build_tree(
        &repo,
        &[("src/a.rs", b"fn a(){}\n"), ("src/b.rs", b"fn b(){}\n")],
    );
    let base_tree = build_tree(&repo, &[("src/a.rs", b"fn a(){}\n")]);
    let base_commit = repo
        .commit(None, &test_sig(), &test_sig(), "base", &base_tree, &[])
        .unwrap();
    commit_tree_to_ref(
        &repo,
        &format!("refs/vendor/{vendor_name}"),
        &upstream_tree,
        "upstream adds b.rs",
    );

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(base_commit.to_string()),
        history: Default::default(),
        patterns: vec!["src/**:ext/".to_string()],
    };

    let idx = repo.merge_vendor(&vendor, None).unwrap();
    assert!(
        !idx.has_conflicts(),
        "adding a new file should merge cleanly"
    );

    let paths: Vec<String> = idx
        .iter()
        .filter(|e| (e.flags >> 12) & 0x3 == 0)
        .map(|e| String::from_utf8_lossy(&e.path).to_string())
        .collect();

    assert!(
        paths.contains(&"ext/b.rs".to_string()),
        "new upstream src/b.rs should appear at ext/b.rs, got: {paths:?}"
    );
    assert!(
        paths.contains(&"ext/a.rs".to_string()),
        "existing ext/a.rs should still be present, got: {paths:?}"
    );
    assert!(
        !paths.contains(&"src/b.rs".to_string()),
        "raw upstream path src/b.rs must not appear in merge index, got: {paths:?}"
    );
}

#[test]
fn test_merge_vendor_includes_new_upstream_files_matching_patterns() {
    let vendor_name = "selective";
    let (repo, tmp, vendor) = setup_merge_scenario(
        vendor_name,
        &[("owned.txt", b"v1\n")],
        &[("owned.txt", b"v2\n"), ("new.txt", b"hello\n")],
        &["**"],
    );

    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
    });

    assert!(
        tmp.path().join("new.txt").exists(),
        "new upstream file matching patterns must be included"
    );
    let content = std::fs::read_to_string(tmp.path().join("owned.txt")).unwrap();
    assert_eq!(content, "v2\n");
    let new_content = std::fs::read_to_string(tmp.path().join("new.txt")).unwrap();
    assert_eq!(new_content, "hello\n");
}

#[test]
fn test_update_stages_gitattributes_when_cwd_differs_from_workdir() {
    let vendor_name = "subdcwd";

    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    // Upstream base.
    let old_tree = build_tree(&repo, &[("lib.rs", b"v1\n")]);
    let old_base_oid = repo
        .commit(None, &test_sig(), &test_sig(), "up v1", &old_tree, &[])
        .unwrap();
    let old_base = repo.find_commit(old_base_oid).unwrap();

    // Upstream update.
    let new_tree = build_tree(&repo, &[("lib.rs", b"v2\n")]);
    repo.commit(
        Some(&format!("refs/vendor/{vendor_name}")),
        &test_sig(),
        &test_sig(),
        "up v2",
        &new_tree,
        &[&old_base],
    )
    .unwrap();

    // Local HEAD with vendor files under ext/ and a subdirectory .gitattributes.
    std::fs::create_dir_all(tmp.path().join("ext")).unwrap();
    std::fs::write(
        tmp.path().join("ext/.gitattributes"),
        format!("lib.rs vendor={vendor_name}\n"),
    )
    .unwrap();
    std::fs::write(tmp.path().join("ext/lib.rs"), b"v1\n").unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(old_base_oid.to_string()),
        history: Default::default(),
        patterns: vec!["**:ext/".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &test_sig(), &test_sig(), "local", &tree, &[])
            .unwrap();
    }

    // Run update from a *different* CWD (the system temp dir, not the repo).
    // Before the fix, gitattributes_rel.exists() checked CWD and would miss
    // the ext/.gitattributes file.
    let other_dir = tempfile::tempdir().unwrap();
    with_cwd(other_dir.path(), || {
        git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
    });

    // The ext/.gitattributes must be staged in the index.
    let index = repo.index().unwrap();
    assert!(
        index.get_path(Path::new("ext/.gitattributes"), 0).is_some(),
        "ext/.gitattributes must be staged even when CWD differs from workdir"
    );
}
