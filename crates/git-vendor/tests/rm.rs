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
    let repo = git2::Repository::init(tmp.path()).unwrap();

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

fn write_gitvendors(tmp: &Path, vendor: &VendorSource) {
    let path = tmp.join(".gitvendors");
    if !path.exists() {
        std::fs::write(&path, "").unwrap();
    }
    let mut cfg = git2::Config::open(&path).unwrap();
    vendor.to_config(&mut cfg).unwrap();
}

#[test]
fn test_rm_removes_gitvendors_entry() {
    let (repo, tmp) = init_repo_with_gitattributes("a.txt vendor=mylib\n");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"content\n")]);
    commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &upstream_tree, "tip");

    std::fs::write(tmp.path().join("a.txt"), b"content\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
    }

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        branch: None,
        base: None,
        commit: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".gitvendors")).unwrap();
        index.write().unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::rm(&repo, "mylib").unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitvendors")).unwrap();
    assert!(
        !content.contains("mylib"),
        ".gitvendors must not reference mylib after rm; got:\n{content}"
    );
}

#[test]
fn test_rm_deletes_vendor_refs() {
    let (repo, tmp) = init_repo_with_gitattributes("a.txt vendor=mylib\n");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"content\n")]);
    let head_oid = commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &upstream_tree, "tip");
    repo.reference("refs/vendor/mylib/base", head_oid, false, "base")
        .unwrap();

    std::fs::write(tmp.path().join("a.txt"), b"content\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
    }

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        branch: None,
        base: None,
        commit: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".gitvendors")).unwrap();
        index.write().unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::rm(&repo, "mylib").unwrap();
    });

    assert!(
        repo.find_reference("refs/vendor/mylib/head").is_err(),
        "refs/vendor/mylib/head must be deleted"
    );
    assert!(
        repo.find_reference("refs/vendor/mylib/base").is_err(),
        "refs/vendor/mylib/base must be deleted"
    );
}

#[test]
fn test_rm_clears_gitattributes() {
    let (repo, tmp) = init_repo_with_gitattributes("a.txt vendor=mylib\nother.txt vendor=other\n");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"content\n")]);
    commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &upstream_tree, "tip");

    std::fs::write(tmp.path().join("a.txt"), b"content\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
    }

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        branch: None,
        base: None,
        commit: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".gitvendors")).unwrap();
        index.write().unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::rm(&repo, "mylib").unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        !content.contains("vendor=mylib"),
        "vendor=mylib must be removed from .gitattributes; got:\n{content}"
    );
    assert!(
        content.contains("vendor=other"),
        "unrelated vendor attr must be preserved; got:\n{content}"
    );
}

#[test]
fn test_rm_stages_vendored_files_as_deleted_by_them() {
    let (repo, tmp) = init_repo_with_gitattributes("a.txt vendor=mylib\n");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"content\n")]);
    commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &upstream_tree, "tip");

    std::fs::write(tmp.path().join("a.txt"), b"content\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
    }

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        branch: None,
        base: None,
        commit: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".gitvendors")).unwrap();
        index.write().unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::rm(&repo, "mylib").unwrap();
    });

    let index = repo.index().unwrap();
    let stages: Vec<u16> = index
        .iter()
        .filter(|e| std::str::from_utf8(&e.path).map_or(false, |p| p == "a.txt"))
        .map(|e| (e.flags >> 12) & 0x3)
        .collect();

    assert!(!stages.contains(&0), "stage-0 entry must be gone after rm");
    assert!(
        stages.contains(&1),
        "stage-1 (ancestor) entry must be present"
    );
    assert!(stages.contains(&2), "stage-2 (ours) entry must be present");
    assert!(
        !stages.contains(&3),
        "stage-3 (theirs) must be absent — deleted by them"
    );
}

#[test]
fn test_rm_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    write_gitvendors(
        tmp.path(),
        &VendorSource {
            name: "real".into(),
            url: "https://example.com/real.git".into(),
            branch: None,
            base: None,
            commit: Default::default(),
            patterns: vec![],
        },
    );
    let result = with_cwd(tmp.path(), || git_vendor::exe::rm(&repo, "ghost"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_prune_removes_refs_not_in_gitvendors() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let tree = build_tree(&repo, &[("f.txt", b"x\n")]);
    commit_tree_to_ref(&repo, "refs/vendor/stale/head", &tree, "stale tip");

    write_gitvendors(
        tmp.path(),
        &VendorSource {
            name: "dummy".into(),
            url: "https://example.com/dummy.git".into(),
            branch: None,
            base: None,
            commit: Default::default(),
            patterns: vec![],
        },
    );
    std::fs::write(tmp.path().join(".gitvendors"), "").unwrap();

    let pruned = with_cwd(tmp.path(), || git_vendor::exe::prune(&repo).unwrap());

    assert!(
        pruned.contains(&"stale".to_string()),
        "stale vendor must be reported as pruned; got {:?}",
        pruned
    );
    assert!(
        repo.find_reference("refs/vendor/stale/head").is_err(),
        "refs/vendor/stale/head must be deleted after prune"
    );
}

#[test]
fn test_prune_keeps_refs_that_are_in_gitvendors() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let tree = build_tree(&repo, &[("f.txt", b"x\n")]);
    let oid = commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &tree, "tip");

    write_gitvendors(
        tmp.path(),
        &VendorSource {
            name: "mylib".into(),
            url: "https://example.com/mylib.git".into(),
            branch: None,
            base: Some(oid.to_string()),
            commit: Default::default(),
            patterns: vec!["**".into()],
        },
    );

    let pruned = with_cwd(tmp.path(), || git_vendor::exe::prune(&repo).unwrap());

    assert!(
        pruned.is_empty(),
        "no refs should be pruned when all match .gitvendors; got {:?}",
        pruned
    );
    assert!(
        repo.find_reference("refs/vendor/mylib/head").is_ok(),
        "refs/vendor/mylib/head must survive prune"
    );
}

#[test]
fn test_prune_also_removes_base_ref() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let tree = build_tree(&repo, &[("f.txt", b"x\n")]);
    let oid = commit_tree_to_ref(&repo, "refs/vendor/old/head", &tree, "tip");
    repo.reference("refs/vendor/old/base", oid, false, "base")
        .unwrap();

    std::fs::write(tmp.path().join(".gitvendors"), "").unwrap();

    let pruned = with_cwd(tmp.path(), || git_vendor::exe::prune(&repo).unwrap());

    assert!(pruned.contains(&"old".to_string()));
    assert!(
        repo.find_reference("refs/vendor/old/head").is_err(),
        "head ref must be deleted"
    );
    assert!(
        repo.find_reference("refs/vendor/old/base").is_err(),
        "base ref must also be deleted"
    );
}

#[test]
fn test_prune_empty_repo_returns_empty() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    std::fs::write(tmp.path().join(".gitvendors"), "").unwrap();

    let pruned = with_cwd(tmp.path(), || git_vendor::exe::prune(&repo).unwrap());
    assert!(pruned.is_empty());
}
