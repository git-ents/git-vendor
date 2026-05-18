//! Table-driven tests for `base_tree`.
//!
//! These tests exercise only the path where no base has been recorded. When a
//! base is present, `base_tree` delegates to `upstream_tree`, which is not yet
//! implemented. What the tests pin down is the documented contract that an
//! absent base means an absent ancestor: the empty tree is the implied
//! ancestor at the merge site, never a value that `base_tree` itself
//! fabricates.

use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorName, VendorRepository as _};

// ── Fixture helpers ───────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(std::process::Stdio::null())
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Initialize a git repo at `dir` with one commit on `main`.
fn make_upstream(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join("hello.txt"), b"hello").unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-m", "init"], dir);
}

/// Initialize a bare local repo that will fetch from upstream.
fn make_local(dir: &Path) -> gix::Repository {
    git(&["init", "--bare", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    gix::open(dir).expect("gix open")
}

fn entry_without_base(url: &str) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "src/**".to_owned(),
            destination: Some("third_party/".to_owned()),
        }],
    }
}

// ── No recorded base ──────────────────────────────────────────────────────────

/// Even after the vendor has been fetched, so that `refs/vendor/<name>` and
/// the upstream objects exist, an absent base still yields `None`. This is the
/// case worth its own test, because `base_tree` must key off the recorded base
/// alone and must never infer an ancestor from the fetched tip.
#[test]
fn base_tree_none_even_after_fetch() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = entry_without_base(upstream.path().to_str().unwrap());

    repo.fetch_vendor(&entry).expect("fetch_vendor");

    assert!(
        repo.base_tree(&entry).expect("base_tree").is_none(),
        "an absent base must stay absent regardless of the fetched tip",
    );
}
