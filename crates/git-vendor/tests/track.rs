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

fn write_gitvendors(tmp: &Path, vendor: &VendorSource) {
    let path = tmp.join(".gitvendors");
    if !path.exists() {
        std::fs::write(&path, "").unwrap();
    }
    let mut cfg = git2::Config::open(&path).unwrap();
    vendor.to_config(&mut cfg).unwrap();
}

#[test]
fn test_track_patterns_adds_new_pattern() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let vendor = VendorSource {
        name: "tp".into(),
        url: "https://example.com/tp.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::track_patterns(&repo, "tp", &["docs/**"]).unwrap();
    });

    let cfg = repo.vendor_config().unwrap();
    let updated = VendorSource::from_config(&cfg, "tp").unwrap().unwrap();
    assert!(
        updated.patterns.contains(&"src/**".to_string()),
        "existing pattern must be preserved"
    );
    assert!(
        updated.patterns.contains(&"docs/**".to_string()),
        "new pattern must be added"
    );
}

#[test]
fn test_track_patterns_is_idempotent() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let vendor = VendorSource {
        name: "tpi".into(),
        url: "https://example.com/tpi.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::track_patterns(&repo, "tpi", &["src/**"]).unwrap();
    });

    let cfg = repo.vendor_config().unwrap();
    let updated = VendorSource::from_config(&cfg, "tpi").unwrap().unwrap();
    assert_eq!(
        updated.patterns.iter().filter(|p| *p == "src/**").count(),
        1,
        "duplicate pattern must not be added"
    );
}

#[test]
fn test_track_patterns_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    write_gitvendors(
        tmp.path(),
        &VendorSource {
            name: "real".into(),
            url: "https://example.com/real.git".into(),
            ref_name: None,
            base: None,
            history: Default::default(),
            patterns: vec![],
        },
    );

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::track_patterns(&repo, "ghost", &["**"])
    });
    assert!(result.is_err(), "expected error for unknown vendor");
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_untrack_patterns_removes_pattern() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let vendor = VendorSource {
        name: "utp".into(),
        url: "https://example.com/utp.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**".into(), "docs/**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_patterns(&repo, "utp", &["docs/**"]).unwrap();
    });

    let cfg = repo.vendor_config().unwrap();
    let updated = VendorSource::from_config(&cfg, "utp").unwrap().unwrap();
    assert!(
        !updated.patterns.contains(&"docs/**".to_string()),
        "removed pattern must be gone"
    );
    assert!(
        updated.patterns.contains(&"src/**".to_string()),
        "other pattern must be preserved"
    );
}

#[test]
fn test_untrack_patterns_noop_for_absent_pattern() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let vendor = VendorSource {
        name: "utpn".into(),
        url: "https://example.com/utpn.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec!["src/**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_patterns(&repo, "utpn", &["nonexistent/**"]).unwrap();
    });

    let cfg = repo.vendor_config().unwrap();
    let updated = VendorSource::from_config(&cfg, "utpn").unwrap().unwrap();
    assert_eq!(updated.patterns, vec!["src/**".to_string()]);
}

#[test]
fn test_untrack_patterns_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    write_gitvendors(
        tmp.path(),
        &VendorSource {
            name: "real".into(),
            url: "https://example.com/real.git".into(),
            ref_name: None,
            base: None,
            history: Default::default(),
            patterns: vec![],
        },
    );

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_patterns(&repo, "ghost", &["**"])
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_track_attrs_writes_vendor_attribute() {
    let (repo, tmp) = init_repo_with_gitattributes("");

    let vendor = VendorSource {
        name: "ta".into(),
        url: "https://example.com/ta.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    write_gitvendors(tmp.path(), &vendor);

    std::fs::write(tmp.path().join("lib.rs"), b"fn main() {}").unwrap();

    with_cwd(tmp.path(), || {
        git_vendor::exe::track_attrs(&repo, "ta", &["lib.rs"]).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        content.contains("lib.rs") && content.contains("vendor=ta"),
        "expected vendor=ta attr for lib.rs; got:\n{content}"
    );
}

#[test]
fn test_track_attrs_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::track_attrs(&repo, "ghost", &["lib.rs"])
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_untrack_attrs_removes_vendor_attribute() {
    let (repo, tmp) = init_repo_with_gitattributes("lib.rs vendor=uta\n");

    let vendor = VendorSource {
        name: "uta".into(),
        url: "https://example.com/uta.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_attrs(&repo, "uta", &["lib.rs"]).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        !content.contains("vendor=uta"),
        "vendor=uta attr must be removed; got:\n{content}"
    );
}

#[test]
fn test_untrack_attrs_does_not_remove_other_vendors() {
    let initial = "lib.rs vendor=a\nlib.rs vendor=b\n";
    let (repo, tmp) = init_repo_with_gitattributes(initial);

    for name in ["a", "b"] {
        write_gitvendors(
            tmp.path(),
            &VendorSource {
                name: name.into(),
                url: format!("https://example.com/{name}.git"),
                ref_name: None,
                base: None,
                history: Default::default(),
                patterns: vec![],
            },
        );
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_attrs(&repo, "a", &["lib.rs"]).unwrap();
    });

    let content = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(
        !content.contains("vendor=a"),
        "vendor=a must be removed; got:\n{content}"
    );
    assert!(
        content.contains("vendor=b"),
        "vendor=b must be preserved; got:\n{content}"
    );
}

#[test]
fn test_untrack_attrs_noop_when_gitattributes_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    {
        std::fs::write(tmp.path().join("x.txt"), b"x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("x.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &test_sig(), &test_sig(), "init", &tree, &[])
            .unwrap();
    }

    let vendor = VendorSource {
        name: "v".into(),
        url: "https://example.com/v.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_attrs(&repo, "v", &["x.txt"]).unwrap();
    });
}

#[test]
fn test_untrack_attrs_errors_on_unknown_vendor() {
    let (repo, tmp) = init_repo_with_gitattributes("");
    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::untrack_attrs(&repo, "ghost", &["lib.rs"])
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ghost"));
}

#[test]
fn test_track_patterns_with_build_tree_helper() {
    // Smoke test to confirm build_tree compiles in this file.
    let (repo, tmp) = init_repo_with_gitattributes("");
    let _tree = build_tree(&repo, &[("a.txt", b"hello")]);
    let _ = tmp;
}
