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

fn write_gitvendors(tmp: &Path, vendor: &VendorSource) {
    let path = tmp.join(".gitvendors");
    if !path.exists() {
        std::fs::write(&path, "").unwrap();
    }
    let mut cfg = git2::Config::open(&path).unwrap();
    vendor.to_config(&mut cfg).unwrap();
}

fn make_bare_upstream(files: &[(&str, &[u8])]) -> (git2::Repository, tempfile::TempDir, git2::Oid) {
    let tmp = tempfile::tempdir().unwrap();
    let bare = git2::Repository::init_bare(tmp.path()).unwrap();
    let oid = {
        let tree = build_tree(&bare, files);
        bare.commit(
            Some("refs/heads/main"),
            &test_sig(),
            &test_sig(),
            "initial",
            &tree,
            &[],
        )
        .unwrap()
    };
    bare.reference_symbolic("HEAD", "refs/heads/main", true, "init")
        .ok();
    (bare, tmp, oid)
}

#[test]
fn test_fetch_one_creates_vendor_ref() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let (_bare, bare_tmp, expected_oid) = make_bare_upstream(&[("a.txt", b"hello\n")]);

    let url = {
        let p = bare_tmp.path().to_string_lossy().replace('\\', "/");
        if p.starts_with('/') {
            format!("file://{p}")
        } else {
            format!("file:///{p}")
        }
    };

    let vendor = VendorSource {
        name: "upstream".into(),
        url: url.clone(),
        ref_name: Some("main".into()),
        base: None,
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::fetch_one(&repo, "upstream").unwrap()
    });

    assert_eq!(
        result,
        Some(expected_oid),
        "fetch_one must return the new tip OID"
    );
    assert!(
        repo.find_reference("refs/vendor/upstream").is_ok(),
        "refs/vendor/upstream must exist after fetch"
    );
    assert_eq!(
        repo.find_reference("refs/vendor/upstream")
            .unwrap()
            .target()
            .unwrap(),
        expected_oid
    );
}

#[test]
fn test_fetch_one_returns_none_when_already_up_to_date() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let (_bare, bare_tmp, expected_oid) = make_bare_upstream(&[("a.txt", b"hello\n")]);

    let url = {
        let p = bare_tmp.path().to_string_lossy().replace('\\', "/");
        if p.starts_with('/') {
            format!("file://{p}")
        } else {
            format!("file:///{p}")
        }
    };

    let vendor = VendorSource {
        name: "up2".into(),
        url: url.clone(),
        ref_name: Some("main".into()),
        base: None,
        history: Default::default(),
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::fetch_one(&repo, "up2").unwrap()
    });

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::fetch_one(&repo, "up2").unwrap()
    });

    assert_eq!(
        result, None,
        "fetch_one must return None when ref did not advance; oid={expected_oid}"
    );
}

#[test]
fn test_fetch_one_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    std::fs::write(tmp.path().join(".gitvendors"), "").unwrap();

    let result = with_cwd(tmp.path(), || git_vendor::exe::fetch_one(&repo, "ghost"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_fetch_all_fetches_every_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let (_b1, b1_tmp, oid1) = make_bare_upstream(&[("a.txt", b"from-a\n")]);
    let (_b2, b2_tmp, oid2) = make_bare_upstream(&[("b.txt", b"from-b\n")]);

    let url = |p: &std::path::Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            format!("file:///{s}")
        }
    };

    for (name, bare_tmp) in [("va", &b1_tmp), ("vb", &b2_tmp)] {
        write_gitvendors(
            tmp.path(),
            &VendorSource {
                name: name.into(),
                url: url(bare_tmp.path()),
                ref_name: Some("main".into()),
                base: None,
                history: Default::default(),
                patterns: vec!["**".into()],
            },
        );
    }

    let results = with_cwd(tmp.path(), || git_vendor::exe::fetch_all(&repo).unwrap());

    assert_eq!(
        results.len(),
        2,
        "fetch_all must return one entry per vendor"
    );

    let mut by_name: std::collections::HashMap<_, _> = results.into_iter().collect();
    assert_eq!(by_name.remove("va"), Some(oid1));
    assert_eq!(by_name.remove("vb"), Some(oid2));
}
