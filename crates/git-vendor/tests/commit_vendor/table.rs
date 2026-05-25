//! Tests for `commit_vendor`.
//!
//! `commit_vendor` mints the merge commit recording a [`VendorMerge`] on top of
//! the local "ours" commit: a two-parent commit whose tree is the merge result,
//! whose first parent is the passed `parent`, and whose second parent is
//! `merge.upstream_commit` — a real edge into the `refs/vendor/<name>` graph. It
//! writes only the commit object: no `HEAD` or branch ref is moved.
//!
//! The fixture mirrors the `merge_vendor` tests: a bare local repo with the
//! local ("ours") commit fetched in and an upstream commit fetched via
//! `fetch_vendor`, then a real merge produced to commit.

use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorMerge, VendorName, VendorRepository as _};
use gix::bstr::ByteSlice as _;

// ── git helpers ───────────────────────────────────────────────────────────────

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

fn git_capture(args: &[&str], dir: &Path) -> Vec<u8> {
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

fn write(dir: &Path, rel: &str, contents: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn init(dir: &Path) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
}

fn sig<'a>(name: &'a str, email: &'a str, time: &'a str) -> gix::actor::SignatureRef<'a> {
    gix::actor::SignatureRef {
        name: name.into(),
        email: email.into(),
        time,
    }
}

fn entry(url: &str) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![PatternMapping {
            glob: "up/**".to_owned(),
            destination: Some("vendor/".to_owned()),
        }],
    }
}

struct Built {
    _bare: tempfile::TempDir,
    _upstream: tempfile::TempDir,
    repo: gix::Repository,
    ours: gix::ObjectId,
    merge: VendorMerge,
}

/// A bare local repo with a clean first-add merge ready to commit: the local
/// ("ours") commit carries `vendor/* vendor=mylib`, the upstream commit is
/// remapped from `up/` to the disjoint `vendor/` namespace.
fn build() -> Built {
    let upstream = tempfile::tempdir().unwrap();
    init(upstream.path());
    write(upstream.path(), "README", b"upstream");
    write(upstream.path(), "up/x.txt", b"upstream\n");
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "upstream"], upstream.path());

    let bare = tempfile::tempdir().unwrap();
    git(&["init", "--bare", "-b", "main"], bare.path());
    let repo = gix::open(bare.path()).expect("gix open");

    // Build the local "ours" commit in a scratch worktree and fetch it in.
    let scratch = tempfile::tempdir().unwrap();
    init(scratch.path());
    write(scratch.path(), "README", b"scratch");
    write(scratch.path(), ".gitattributes", b"vendor/* vendor=mylib\n");
    write(scratch.path(), "vendor/keep.txt", b"local\n");
    git(&["add", "-A"], scratch.path());
    git(&["commit", "-m", "scratch"], scratch.path());
    let hex = git_capture(&["rev-parse", "HEAD"], scratch.path());
    let ours = gix::ObjectId::from_hex(hex.trim()).expect("commit oid");
    git(
        &[
            "fetch",
            scratch.path().to_str().unwrap(),
            "+HEAD:refs/scratch/ours",
        ],
        bare.path(),
    );

    let e = entry(upstream.path().to_str().unwrap());
    let theirs = repo.fetch_vendor(&e).expect("fetch_vendor");
    let merge = repo.merge_vendor(&e, ours, theirs).expect("merge_vendor");
    assert!(merge.conflicts.is_empty(), "{:?}", merge.conflicts);

    Built {
        _bare: bare,
        _upstream: upstream,
        repo,
        ours,
        merge,
    }
}

/// The minted commit's tree is `merge.result_tree`, and its parents are exactly
/// `[parent, merge.upstream_commit]` in that order — the local "ours" commit
/// first, the upstream commit second.
#[test]
fn mints_two_parent_merge_commit() {
    let b = build();
    let e = entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig("Committer", "c@example.com", "1700000000 +0000"),
            sig("Author", "a@example.com", "1700000000 +0000"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let commit = b.repo.find_commit(oid).expect("find_commit");
    let decoded = commit.decode().expect("decode");

    assert_eq!(decoded.tree(), b.merge.result_tree);
    let parents: Vec<gix::ObjectId> = decoded.parents().collect();
    assert_eq!(parents, vec![b.ours, b.merge.upstream_commit]);
}

/// The author, committer, and message are recorded verbatim; author and
/// committer are independent.
#[test]
fn records_author_committer_and_message() {
    let b = build();
    let e = entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig(
                "Committer Name",
                "committer@example.com",
                "1700000123 +0100",
            ),
            sig("Author Name", "author@example.com", "1699999999 -0500"),
            "vendor: integrate mylib upstream",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let commit = b.repo.find_commit(oid).expect("find_commit");
    let decoded = commit.decode().expect("decode");

    let author = decoded.author().expect("author");
    let committer = decoded.committer().expect("committer");
    assert_eq!(author.name, "Author Name");
    assert_eq!(author.email, "author@example.com");
    assert_eq!(committer.name, "Committer Name");
    assert_eq!(committer.email, "committer@example.com");
    assert_eq!(
        decoded.message.as_bstr(),
        b"vendor: integrate mylib upstream".as_bstr(),
    );
}

/// `commit_vendor` writes only the commit object: no reference is created or
/// moved, and the new commit is pointed to by nothing.
#[test]
fn does_not_move_any_ref() {
    let b = build();
    let e = entry("unused");

    let before: Vec<(String, gix::ObjectId)> = b
        .repo
        .references()
        .unwrap()
        .all()
        .unwrap()
        .filter_map(Result::ok)
        .map(|mut r| {
            (
                r.name().as_bstr().to_string(),
                r.peel_to_id().unwrap().detach(),
            )
        })
        .collect();

    let oid = b
        .repo
        .commit_vendor(
            sig("C", "c@example.com", "1700000000 +0000"),
            sig("A", "a@example.com", "1700000000 +0000"),
            "merge",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let after: Vec<(String, gix::ObjectId)> = b
        .repo
        .references()
        .unwrap()
        .all()
        .unwrap()
        .filter_map(Result::ok)
        .map(|mut r| {
            (
                r.name().as_bstr().to_string(),
                r.peel_to_id().unwrap().detach(),
            )
        })
        .collect();

    assert_eq!(before, after, "commit_vendor must not move or create refs");
    assert!(
        !after.iter().any(|(_, id)| *id == oid),
        "no ref may point at the freshly minted commit",
    );
}
