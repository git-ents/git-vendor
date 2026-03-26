mod common;

use git_vendor::{History, VendorSource};
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

fn setup_commit_mode_scenario(
    vendor_name: &str,
    commit_mode: History,
) -> (
    git2::Repository,
    tempfile::TempDir,
    VendorSource,
    git2::Oid,
    git2::Oid,
) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let old_base_oid = {
        let old_upstream_tree = build_tree(&repo, &[("a.txt", b"v1\n")]);
        repo.commit(
            None,
            &test_sig(),
            &test_sig(),
            "upstream v1",
            &old_upstream_tree,
            &[],
        )
        .unwrap()
    };

    let attrs = format!("a.txt vendor={vendor_name}\n");
    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"v1\n").unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: Some("main".into()),
        base: Some(old_base_oid.to_string()),
        history: commit_mode,
        patterns: vec!["**".into()],
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
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "local HEAD",
            &tree,
            &[],
        )
        .unwrap();
    }

    let new_head_oid = {
        let new_upstream_tree = build_tree(&repo, &[("a.txt", b"v1\nv2\n")]);
        repo.commit(
            Some(&format!("refs/vendor/{vendor_name}")),
            &test_sig(),
            &test_sig(),
            "upstream v2",
            &new_upstream_tree,
            &[],
        )
        .unwrap()
    };

    (repo, tmp, vendor, old_base_oid, new_head_oid)
}

#[test]
fn test_commit_mode_linear_creates_single_parent_commit() {
    let (repo, tmp, _vendor, _old_base, _new_head) =
        setup_commit_mode_scenario("lin", History::Linear);

    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "lin", None, false).unwrap();
    });

    let head_after = repo.head().unwrap().peel_to_commit().unwrap();
    assert_ne!(
        head_after.id(),
        head_before,
        "a new commit should be created"
    );
    assert_eq!(
        head_after.parent_count(),
        1,
        "linear mode must produce a single-parent commit"
    );
    assert_eq!(
        head_after.parent(0).unwrap().id(),
        head_before,
        "parent must be the old HEAD"
    );
}

#[test]
fn test_commit_mode_squash_creates_merge_commit() {
    let (repo, tmp, _vendor, _old_base, _new_head) =
        setup_commit_mode_scenario("sq", History::Squash);

    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "sq", None, false).unwrap();
    });

    let head_after = repo.head().unwrap().peel_to_commit().unwrap();
    assert_ne!(head_after.id(), head_before);
    assert_eq!(
        head_after.parent_count(),
        2,
        "squash mode must produce a two-parent merge commit"
    );
    assert_eq!(
        head_after.parent(0).unwrap().id(),
        head_before,
        "first parent must be the old HEAD"
    );
}

#[test]
fn test_commit_mode_replay_creates_one_commit_per_upstream() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "replay";

    let old_tree = build_tree(&repo, &[("a.txt", b"v1\n")]);
    let old_base_oid = repo
        .commit(None, &test_sig(), &test_sig(), "up v1", &old_tree, &[])
        .unwrap();
    let old_base_commit = repo.find_commit(old_base_oid).unwrap();

    let mid_tree = build_tree(&repo, &[("a.txt", b"v1\nv2\n")]);
    let mid_oid = repo
        .commit(
            None,
            &test_sig(),
            &test_sig(),
            "up v2",
            &mid_tree,
            &[&old_base_commit],
        )
        .unwrap();
    let mid_commit = repo.find_commit(mid_oid).unwrap();

    let new_tree = build_tree(&repo, &[("a.txt", b"v1\nv2\nv3\n")]);
    let new_head_oid = repo
        .commit(
            Some(&format!("refs/vendor/{vendor_name}")),
            &test_sig(),
            &test_sig(),
            "up v3",
            &new_tree,
            &[&mid_commit],
        )
        .unwrap();
    let _ = new_head_oid;

    let attrs = format!("a.txt vendor={vendor_name}\n");
    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"v1\n").unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(old_base_oid.to_string()),
        history: History::Replay,
        patterns: vec!["**".into()],
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
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "local HEAD",
            &tree,
            &[],
        )
        .unwrap();
    }

    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
    });

    let head_after = repo.head().unwrap().peel_to_commit().unwrap();
    assert_ne!(head_after.id(), head_before);

    let commit2 = head_after.clone();
    let commit1 = commit2.parent(0).unwrap();
    assert_eq!(
        commit1.parent(0).unwrap().id(),
        head_before,
        "two commits ago must be the old HEAD"
    );
    assert_eq!(commit2.summary(), Some("up v3"));
    assert_eq!(commit1.summary(), Some("up v2"));
}

#[test]
fn test_commit_mode_replay_preserves_non_vendor_files() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "repkeep";

    // Upstream base commit.
    let old_tree = build_tree(&repo, &[("lib.txt", b"v1\n")]);
    let old_base_oid = repo
        .commit(None, &test_sig(), &test_sig(), "up v1", &old_tree, &[])
        .unwrap();
    let old_base_commit = repo.find_commit(old_base_oid).unwrap();

    // Upstream update.
    let new_tree = build_tree(&repo, &[("lib.txt", b"v2\n")]);
    repo.commit(
        Some(&format!("refs/vendor/{vendor_name}")),
        &test_sig(),
        &test_sig(),
        "up v2",
        &new_tree,
        &[&old_base_commit],
    )
    .unwrap();

    // Local HEAD has both the vendor file and a non-vendor file.
    let attrs = format!("lib.txt vendor={vendor_name}\n");
    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();
    std::fs::write(tmp.path().join("lib.txt"), b"v1\n").unwrap();
    std::fs::write(tmp.path().join("local.txt"), b"keep me\n").unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(old_base_oid.to_string()),
        history: History::Replay,
        patterns: vec!["**".into()],
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
        repo.commit(Some("HEAD"), &test_sig(), &test_sig(), "local HEAD", &tree, &[])
            .unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
    });

    // The replayed commit must contain both the updated vendor file
    // and the untouched local file.
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let head_tree = head.tree().unwrap();

    assert!(
        head_tree.get_path(Path::new("local.txt")).is_ok(),
        "non-vendor file must survive replay"
    );
    assert!(
        head_tree.get_path(Path::new("lib.txt")).is_ok(),
        "vendor file must be present after replay"
    );

    // Verify content.
    let blob = head_tree
        .get_path(Path::new("lib.txt"))
        .unwrap()
        .to_object(&repo)
        .unwrap();
    assert_eq!(blob.as_blob().unwrap().content(), b"v2\n");

    let local_blob = head_tree
        .get_path(Path::new("local.txt"))
        .unwrap()
        .to_object(&repo)
        .unwrap();
    assert_eq!(local_blob.as_blob().unwrap().content(), b"keep me\n");
}

#[test]
fn test_commit_mode_replay_preserves_author_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "repauth";

    let old_tree = build_tree(&repo, &[("f.txt", b"old\n")]);
    let old_base_oid = repo
        .commit(None, &test_sig(), &test_sig(), "base", &old_tree, &[])
        .unwrap();
    let old_base_commit = repo.find_commit(old_base_oid).unwrap();

    let upstream_author = git2::Signature::new(
        "Alice Upstream",
        "alice@upstream.org",
        &git2::Time::new(1_700_000_000, 0),
    )
    .unwrap();
    let new_tree = build_tree(&repo, &[("f.txt", b"new\n")]);
    repo.commit(
        Some(&format!("refs/vendor/{vendor_name}")),
        &upstream_author,
        &upstream_author,
        "upstream change",
        &new_tree,
        &[&old_base_commit],
    )
    .unwrap();

    let attrs = format!("f.txt vendor={vendor_name}\n");
    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();
    std::fs::write(tmp.path().join("f.txt"), b"old\n").unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(old_base_oid.to_string()),
        history: History::Replay,
        patterns: vec!["**".into()],
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
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "local HEAD",
            &tree,
            &[],
        )
        .unwrap();
    }

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
    });

    let replayed = repo.head().unwrap().peel_to_commit().unwrap();
    let author = replayed.author();
    assert_eq!(author.name(), Some("Alice Upstream"));
    assert_eq!(author.email(), Some("alice@upstream.org"));
    assert_eq!(author.when().seconds(), 1_700_000_000);
}

#[test]
fn test_no_commit_writes_vendor_msg_and_does_not_commit() {
    let (repo, tmp, _vendor, _old_base, _new_head) =
        setup_commit_mode_scenario("nc", History::Linear);

    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "nc", None, true).unwrap();
    });

    let head_after = repo.head().unwrap().peel_to_commit().unwrap().id();
    assert_eq!(
        head_after, head_before,
        "--no-commit must not create a commit"
    );

    let msg_path = repo.path().join("VENDOR_MSG");
    assert!(
        msg_path.exists(),
        "VENDOR_MSG must be written for --no-commit"
    );
    let msg = std::fs::read_to_string(&msg_path).unwrap();
    assert!(
        msg.contains("Vendor update:"),
        "VENDOR_MSG must contain subject line; got:\n{msg}"
    );
}

#[test]
fn test_vendor_msg_format_contains_required_sections() {
    let (repo, tmp, _vendor, _old_base_oid, _new_head_oid) =
        setup_commit_mode_scenario("vmf", History::Linear);

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "vmf", None, true).unwrap();
    });

    let msg_path = repo.path().join("VENDOR_MSG");
    let msg = std::fs::read_to_string(&msg_path).unwrap();

    assert!(
        msg.starts_with("Vendor update: vmf main"),
        "subject must start with 'Vendor update: vmf main'; got:\n{msg}"
    );
    assert!(
        msg.contains("Updated") && msg.contains("added") && msg.contains("removed"),
        "body must contain file stats; got:\n{msg}"
    );
    assert!(
        msg.contains("Upstream-Author:"),
        "must contain Upstream-Author trailer; got:\n{msg}"
    );
}

#[test]
fn test_conflict_vendor_msg_contains_resolution_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::init_test_repo(tmp.path());

    let vendor_name = "confmsg";

    let attrs = format!("f.txt vendor={vendor_name}\n");
    std::fs::write(tmp.path().join(".gitattributes"), &attrs).unwrap();
    std::fs::write(tmp.path().join("f.txt"), b"original\n").unwrap();

    let base_oid = {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let base_upstream_tree = build_tree(&repo, &[("f.txt", b"original\n")]);
        let base_commit_oid = repo
            .commit(
                None,
                &test_sig(),
                &test_sig(),
                "base upstream",
                &base_upstream_tree,
                &[],
            )
            .unwrap();
        repo.commit(
            Some("HEAD"),
            &test_sig(),
            &test_sig(),
            "local base",
            &tree,
            &[],
        )
        .unwrap();
        base_commit_oid
    };

    std::fs::write(tmp.path().join("f.txt"), b"local edit\n").unwrap();
    let local_head = repo.head().unwrap().peel_to_commit().unwrap();
    commit_workdir(&repo, "local edit", &[&local_head]);

    let upstream_tree = build_tree(&repo, &[("f.txt", b"upstream edit\n")]);
    repo.commit(
        Some(&format!("refs/vendor/{vendor_name}")),
        &test_sig(),
        &test_sig(),
        "upstream edit",
        &upstream_tree,
        &[],
    )
    .unwrap();

    let vendor = VendorSource {
        name: vendor_name.to_string(),
        url: "https://example.com/upstream.git".into(),
        ref_name: None,
        base: Some(base_oid.to_string()),
        history: History::Linear,
        patterns: vec!["**".into()],
    };
    write_gitvendors(tmp.path(), &vendor);

    with_cwd(tmp.path(), || {
        let outcome = git_vendor::exe::update_one(&repo, vendor_name, None, false).unwrap();
        assert!(
            matches!(outcome, git_vendor::exe::MergeOutcome::Conflict { .. }),
            "expected conflict outcome"
        );
    });

    let msg_path = repo.path().join("VENDOR_MSG");
    assert!(msg_path.exists(), "VENDOR_MSG must be written on conflict");
    let msg = std::fs::read_to_string(&msg_path).unwrap();
    assert!(
        msg.contains("git commit -e -F .git/VENDOR_MSG"),
        "conflict VENDOR_MSG must contain resolution instructions; got:\n{msg}"
    );
}

#[test]
fn test_base_written_after_staging_not_before_merge() {
    let (repo, tmp, vendor, old_base_oid, _new_head) =
        setup_commit_mode_scenario("bwrt", History::Linear);

    let gitvendors_before = std::fs::read_to_string(tmp.path().join(".gitvendors")).unwrap();
    assert!(
        gitvendors_before.contains(&old_base_oid.to_string()),
        "pre-merge .gitvendors must contain old base"
    );
    let _ = vendor;

    with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "bwrt", None, false).unwrap();
    });

    let gitvendors_after = std::fs::read_to_string(tmp.path().join(".gitvendors")).unwrap();
    assert!(
        !gitvendors_after.contains(&old_base_oid.to_string()),
        "post-merge .gitvendors must not still contain old base"
    );
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let head_parent_id = head.parent(0).unwrap().id();
    let head_tree = head.tree().unwrap();
    let gv_entry = head_tree
        .get_path(std::path::Path::new(".gitvendors"))
        .unwrap();
    let gv_blob = repo.find_blob(gv_entry.id()).unwrap();
    let gv_content = std::str::from_utf8(gv_blob.content()).unwrap();
    assert!(
        !gv_content.contains(&old_base_oid.to_string()),
        "committed .gitvendors must reflect updated base"
    );
    let _ = head_parent_id;
}

#[test]
fn test_no_commit_with_replay_returns_error() {
    let (repo, tmp, _vendor, _old_base, _new_head) =
        setup_commit_mode_scenario("ncr", History::Replay);

    let result = with_cwd(tmp.path(), || {
        git_vendor::exe::update_one(&repo, "ncr", None, true)
    });

    assert!(
        result.is_err(),
        "--no-commit must be rejected when commit mode is Replay"
    );
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("replay") || msg.contains("--no-commit"),
        "error message must mention the incompatibility; got: {msg}"
    );
}
