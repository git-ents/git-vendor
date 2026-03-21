use git_vendor::VendorSource;
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
fn test_status_up_to_date() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"v1\n")]);
    let upstream_oid = commit_tree_to_ref(
        &repo,
        "refs/vendor/mylib/head",
        &upstream_tree,
        "upstream tip",
    );

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        ref_name: None,
        base: Some(upstream_oid.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    let statuses = with_cwd(tmp.path(), || git_vendor::exe::check(&repo).unwrap());
    assert_eq!(statuses.len(), 1);
    assert!(
        matches!(statuses[0].state, git_vendor::exe::VendorState::UpToDate),
        "expected UpToDate; got {:?}",
        statuses[0].state
    );
}

#[test]
fn test_status_update_available() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let old_tree = build_tree(&repo, &[("a.txt", b"v1\n")]);
    let old_oid = commit_tree_to_ref(&repo, "refs/vendor/mylib/head", &old_tree, "base commit");

    let new_tree = build_tree(&repo, &[("a.txt", b"v2\n")]);
    let old_commit = repo.find_commit(old_oid).unwrap();
    let new_oid = repo
        .commit(
            Some("refs/vendor/mylib/head"),
            &test_sig(),
            &test_sig(),
            "upstream v2",
            &new_tree,
            &[&old_commit],
        )
        .unwrap();

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        ref_name: None,
        base: Some(old_oid.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    let statuses = with_cwd(tmp.path(), || git_vendor::exe::check(&repo).unwrap());
    assert_eq!(statuses.len(), 1);
    assert!(
        matches!(
            statuses[0].state,
            git_vendor::exe::VendorState::UpdateAvailable { head } if head == new_oid
        ),
        "expected UpdateAvailable; got {:?}",
        statuses[0].state
    );
}

#[test]
fn test_status_update_available_no_base() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let upstream_tree = build_tree(&repo, &[("a.txt", b"v1\n")]);
    let upstream_oid = commit_tree_to_ref(
        &repo,
        "refs/vendor/mylib/head",
        &upstream_tree,
        "upstream tip",
    );

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    let statuses = with_cwd(tmp.path(), || git_vendor::exe::check(&repo).unwrap());
    assert_eq!(statuses.len(), 1);
    assert!(
        matches!(
            statuses[0].state,
            git_vendor::exe::VendorState::UpdateAvailable { head } if head == upstream_oid
        ),
        "expected UpdateAvailable (no base); got {:?}",
        statuses[0].state
    );
}

#[test]
fn test_status_force_pushed() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let tree_a = build_tree(&repo, &[("a.txt", b"v1\n")]);
    let base_oid = commit_tree_to_ref(&repo, "refs/tmp/base", &tree_a, "original upstream");

    let tree_b = build_tree(&repo, &[("a.txt", b"rewritten\n")]);
    let new_head_oid = commit_tree_to_ref(
        &repo,
        "refs/vendor/mylib/head",
        &tree_b,
        "force-pushed upstream",
    );

    let vendor = VendorSource {
        name: "mylib".into(),
        url: "https://example.com/mylib.git".into(),
        ref_name: None,
        base: Some(base_oid.to_string()),
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    let statuses = with_cwd(tmp.path(), || git_vendor::exe::check(&repo).unwrap());
    assert_eq!(statuses.len(), 1);
    assert!(
        matches!(
            statuses[0].state,
            git_vendor::exe::VendorState::ForcePushed { head } if head == new_head_oid
        ),
        "expected ForcePushed; got {:?}",
        statuses[0].state
    );
}

#[test]
fn test_status_sorted_by_name() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    for name in ["zzz", "aaa", "mmm"] {
        let tree = build_tree(&repo, &[("f.txt", b"x\n")]);
        let oid = commit_tree_to_ref(&repo, &format!("refs/vendor/{name}/head"), &tree, "tip");
        write_gitvendors(
            tmp.path(),
            &VendorSource {
                name: name.into(),
                url: format!("https://example.com/{name}.git"),
                ref_name: None,
                base: Some(oid.to_string()),
                history: Default::default(),
                patterns: vec!["**".into()],
            },
        );
    }

    let statuses = with_cwd(tmp.path(), || git_vendor::exe::check(&repo).unwrap());
    let names: Vec<&str> = statuses.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
}
