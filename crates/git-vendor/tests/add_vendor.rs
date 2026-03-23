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

fn make_upstream(files: &[(&str, &[u8])]) -> (git2::Repository, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let bare = git2::Repository::init_bare(tmp.path()).unwrap();
    {
        let tree = build_tree(&bare, files);
        bare.commit(
            Some("refs/heads/main"),
            &test_sig(),
            &test_sig(),
            "initial",
            &tree,
            &[],
        )
        .unwrap();
    }
    (bare, tmp)
}

#[test]
fn test_refresh_vendor_attrs_uses_forward_slashes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

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

    let mut theirs_index = git2::Index::new().unwrap();
    let blob_oid = repo.blob(b"hello").unwrap();
    theirs_index
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
    let theirs_tree_oid = theirs_index.write_tree_to(&repo).unwrap();
    let theirs_tree = repo.find_tree(theirs_tree_oid).unwrap();

    let vendor = VendorSource {
        name: "fwdslash".into(),
        url: "https://example.com/fwdslash.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["**".into()],
    };

    with_cwd(tmp.path(), || {
        repo.refresh_vendor_attrs(&vendor, &theirs_tree)
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        content.contains("sub/file.txt"),
        "expected forward-slash path sub/file.txt in:\n{content}"
    );
    assert!(
        !content.contains('\\'),
        "gitattributes must not contain backslashes:\n{content}"
    );
}

#[test]
fn test_add_vendor_nested_paths_match_correctly() {
    let (repo, tmp) = init_repo_with_gitattributes("");

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

    let upstream_tree = build_tree(&repo, &[("sub/overlap.c", b"// upstream v1")]);
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
        patterns: vec!["sub/".into()],
    };

    with_cwd(tmp.path(), || {
        let index = repo.add_vendor(&vendor, None).unwrap();

        let paths: Vec<String> = index
            .iter()
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p == "sub/overlap.c"),
            "nested file sub/overlap.c should be in merge index, got: {paths:?}"
        );
    });
}

#[test]
fn test_add_vendor_multi_pattern_mixed_mapped_unmapped() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let upstream_tree = build_tree(
        &repo,
        &[
            ("src/main.rs", b"fn main(){}"),
            ("docs/guide.md", b"# Guide"),
        ],
    );
    commit_tree_to_ref(
        &repo,
        "refs/vendor/mixed",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "mixed".into(),
        url: "https://example.com/mixed.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**:ext/".into(), "docs/**:ext/".into()],
    };

    with_cwd(tmp.path(), || {
        let index = repo.add_vendor(&vendor, None).unwrap();
        let paths: Vec<String> = index
            .iter()
            .filter(|e| (e.flags >> 12) & 0x3 == 0)
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .collect();
        assert!(
            paths.contains(&"ext/main.rs".to_string()),
            "src/main.rs should be remapped to ext/main.rs, got: {paths:?}"
        );
        assert!(
            paths.contains(&"ext/guide.md".to_string()),
            "docs/guide.md should be remapped to ext/guide.md, got: {paths:?}"
        );
        assert!(
            !paths.contains(&"src/main.rs".to_string()),
            "raw src/main.rs must not appear in merge index, got: {paths:?}"
        );
    });
}

#[test]
fn test_add_vendor_glob_filtering_with_mapping() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let upstream_tree = build_tree(
        &repo,
        &[
            ("src/lib.rs", b"pub fn lib(){}"),
            ("src/README.txt", b"hello"),
            ("other/data.bin", b"\x00\x01\x02"),
        ],
    );
    commit_tree_to_ref(
        &repo,
        "refs/vendor/filter",
        &upstream_tree,
        "vendor tip",
    );

    let vendor = VendorSource {
        name: "filter".into(),
        url: "https://example.com/filter.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**/*.rs:ext/".into()],
    };

    with_cwd(tmp.path(), || {
        let index = repo.add_vendor(&vendor, None).unwrap();
        let paths: Vec<String> = index
            .iter()
            .filter(|e| (e.flags >> 12) & 0x3 == 0)
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .collect();
        assert!(
            paths.contains(&"ext/lib.rs".to_string()),
            "src/lib.rs should be remapped to ext/lib.rs (literal prefix 'src/' stripped), got: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("README.txt")),
            "README.txt should be excluded, got: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("data.bin")),
            "data.bin should be excluded, got: {paths:?}"
        );
    });
}

#[test]
fn test_refresh_vendor_attrs_ordering_is_consistent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let initial_attrs = "\
.github/workflows/CD.yml vendor=myvendor
.github/workflows/CI.yml vendor=myvendor
.config/committed.toml vendor=myvendor
.config/deny.toml vendor=myvendor
";
    std::fs::write(tmp.path().join(".gitattributes"), initial_attrs).unwrap();

    std::fs::create_dir_all(tmp.path().join(".github/workflows")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".config")).unwrap();
    std::fs::write(tmp.path().join(".github/workflows/CD.yml"), "cd").unwrap();
    std::fs::write(tmp.path().join(".github/workflows/CI.yml"), "ci").unwrap();
    std::fs::write(tmp.path().join(".config/committed.toml"), "c").unwrap();
    std::fs::write(tmp.path().join(".config/deny.toml"), "d").unwrap();

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

    let mut theirs_index = git2::Index::new().unwrap();
    for (path, content) in [
        (".config/committed.toml", b"c" as &[u8]),
        (".config/deny.toml", b"d"),
        (".github/workflows/CD.yml", b"cd"),
        (".github/workflows/CI.yml", b"ci"),
    ] {
        let blob_oid = repo.blob(content).unwrap();
        theirs_index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: blob_oid,
                flags: 0,
                flags_extended: 0,
                path: path.as_bytes().to_vec(),
            })
            .unwrap();
    }
    let theirs_tree_oid = theirs_index.write_tree_to(&repo).unwrap();
    let theirs_tree = repo.find_tree(theirs_tree_oid).unwrap();

    let vendor = VendorSource {
        name: "myvendor".into(),
        url: "https://example.com/myvendor.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["**".into()],
    };

    with_cwd(tmp.path(), || {
        repo.refresh_vendor_attrs(&vendor, &theirs_tree)
            .unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    let attr_lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .collect();

    let mut sorted = attr_lines.clone();
    sorted.sort();
    assert_eq!(
        attr_lines, sorted,
        "gitattributes entries must be sorted; got:\n{content}"
    );

    with_cwd(tmp.path(), || {
        repo.refresh_vendor_attrs(&vendor, &theirs_tree)
            .unwrap();
    });

    let content2 = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert_eq!(content, content2, "refresh_vendor_attrs must be idempotent");
}

#[test]
fn test_add_detects_overlapping_output_paths() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let (_up1, up1_tmp) = make_upstream(&[("a.txt", b"first vendor")]);
    let (_up2, up2_tmp) = make_upstream(&[("a.txt", b"second vendor")]);

    let url1 = up1_tmp.path().to_str().unwrap().to_string();
    let url2 = up2_tmp.path().to_str().unwrap().to_string();

    with_cwd(tmp.path(), || {
        git_vendor::exe::add(&repo, "first", &url1, Some("main"), &["**"], None, None).unwrap();
    });

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::add(&repo, "second", &url2, Some("main"), &["**"], None, None)
    });
    assert!(
        result.is_err(),
        "expected error for overlapping output paths"
    );
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("overlapping output paths"),
        "expected overlap error; got: {msg}"
    );
}

#[test]
fn test_add_detects_collision_with_non_vendored_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    std::fs::write(tmp.path().join(".gitattributes"), "").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), b"// existing\n").unwrap();
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
            "initial with lib.rs",
            &tree,
            &[],
        )
        .unwrap();
    }

    let (_upstream, up_tmp) = make_upstream(&[("lib.rs", b"// upstream\n")]);
    let url = up_tmp.path().to_str().unwrap().to_string();

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::add(&repo, "coll", &url, Some("main"), &["**"], None, None)
    });
    assert!(
        result.is_err(),
        "expected error for collision with non-vendored file"
    );
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("already exists") && msg.contains("not vendored"),
        "expected collision error; got: {msg}"
    );
}

#[test]
fn test_add_vendor_upstream_gitattributes_does_not_clobber_tracking() {
    // Use --path ext/ so vendored files land in ext/, including the upstream
    // .gitattributes at ext/.gitattributes — which is also where vendor
    // tracking entries are written (common dest dir = "ext").
    let (repo, tmp) = init_repo_with_gitattributes("");

    let (_upstream, up_tmp) = make_upstream(&[
        ("lib.rs", b"// code\n"),
        (".gitattributes", b"*.bin binary\n"),
    ]);
    let url = up_tmp.path().to_str().unwrap().to_string();

    with_cwd(tmp.path(), || {
        git_vendor::exe::add(
            &repo,
            "withattr",
            &url,
            Some("main"),
            &["**"],
            Some(Path::new("ext")),
            None,
        )
        .unwrap();
    });

    let content =
        std::fs::read_to_string(tmp.path().join("ext/.gitattributes")).unwrap();
    assert!(
        content.contains("vendor=withattr"),
        "vendor tracking entries must survive; got:\n{content}"
    );
    assert!(
        content.contains("lib.rs"),
        "lib.rs must be tracked; got:\n{content}"
    );
}
