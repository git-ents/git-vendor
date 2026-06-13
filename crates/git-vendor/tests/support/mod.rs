//! Shared test helpers used across integration-test binaries.
//!
//! Each binary declares `mod support;` in its entry file, which compiles this
//! module as a private submodule. Functions are `pub` so sub-modules within
//! each binary can reach them as `crate::support::*`.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

// ── git process helpers ───────────────────────────────────────────────────────

/// Run git with isolated config, assert success, discard stdout.
pub fn git(args: &[&str], dir: &Path) {
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

/// Run git with isolated config, assert success, return stdout bytes.
pub fn git_capture(args: &[&str], dir: &Path) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    output.stdout
}

/// Run the compiled `git-vendor` binary in `dir` with isolated config and
/// return its captured [`Output`](std::process::Output) without asserting
/// success, so tests can inspect both exit status and streams.
pub fn vendor(args: &[&str], dir: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_git-vendor"))
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git-vendor")
}

/// `git rev-parse <rev>` in `dir`, return the resolved OID.
pub fn rev_parse(dir: &Path, rev: &str) -> gix::ObjectId {
    let out = git_capture(&["rev-parse", rev], dir);
    let hex = std::str::from_utf8(&out).unwrap().trim();
    gix::ObjectId::from_hex(hex.as_bytes()).expect("valid hex")
}

// ── filesystem helpers ────────────────────────────────────────────────────────

/// Write `contents` to `dir/rel`, creating parent directories as needed.
pub fn write(dir: &Path, rel: &str, contents: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

// ── repo setup helpers ────────────────────────────────────────────────────────

/// `git init -b main` + minimal user config.
pub fn init(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    git(&["config", "core.autocrlf", "input"], dir);
}

/// `git add -A && git commit -m init`, open with gix, return (repo, HEAD oid).
pub fn commit(dir: &Path) -> (gix::Repository, gix::ObjectId) {
    git(&["add", "-A"], dir);
    git(&["commit", "-m", "init"], dir);
    let repo = gix::open(dir).expect("gix open");
    let head = repo.head_commit().expect("head commit").id().detach();
    (repo, head)
}

/// Initialize a non-bare upstream repo with one commit on `main` (`hello.txt`),
/// return HEAD oid.
pub fn make_upstream(dir: &Path) -> gix::ObjectId {
    init(dir);
    push_commit(dir, b"hello")
}

/// Write `content` to `hello.txt` in an already-initialized repo, commit it,
/// and return the new HEAD oid. Used to advance upstream history in tests.
pub fn push_commit(dir: &Path, content: &[u8]) -> gix::ObjectId {
    std::fs::write(dir.join("hello.txt"), content).unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-m", "c"], dir);
    rev_parse(dir, "HEAD")
}

/// Initialize a bare local repo at `dir` with user config, return gix handle.
pub fn make_local(dir: &Path) -> gix::Repository {
    git(&["init", "--bare", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    gix::open(dir).expect("gix open")
}

/// Create a fresh temporary bare repo, return `(TempDir, Repository)`.
///
/// The `TempDir` must be kept alive as long as the repo is used.
pub fn make_bare() -> (tempfile::TempDir, gix::Repository) {
    let dir = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], dir.path());
    let repo = gix::open(dir.path()).expect("gix open");
    (dir, repo)
}

// ── tree helpers ──────────────────────────────────────────────────────────────

/// Flatten a tree into a `path -> blob oid` map, skipping sub-trees.
pub fn tree_entries(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> BTreeMap<String, gix::ObjectId> {
    use gix::bstr::ByteSlice as _;
    let tree = repo.find_tree(tree).expect("find tree");
    tree.traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .filter(|r| !r.mode.is_tree())
        .map(|r| (r.filepath.to_str_lossy().into_owned(), r.oid))
        .collect()
}

/// Ordered path list from a tree (calls [`tree_entries`] internally).
pub fn paths(repo: &gix::Repository, tree: gix::ObjectId) -> Vec<String> {
    tree_entries(repo, tree).into_keys().collect()
}

/// Build a tree in `repo`'s object database from `(path, bytes)` pairs.
pub fn build_tree(repo: &gix::Repository, files: &[(&str, &[u8])]) -> gix::ObjectId {
    let mut editor = repo.empty_tree().edit().unwrap();
    for (path, bytes) in files {
        let blob = repo.write_blob(bytes).unwrap().detach();
        editor
            .upsert(*path, gix::objs::tree::EntryKind::Blob, blob)
            .unwrap();
    }
    editor.write().unwrap().detach()
}
