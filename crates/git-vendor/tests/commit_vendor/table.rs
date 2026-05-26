//! Tests for `commit_vendor`.
//!
//! `commit_vendor` mints the merge commit recording a [`VendorMerge`] on top of
//! the local "ours" commit: a two-parent commit whose tree is the vendor-only
//! merge result spliced into the full `parent` tree (non-vendored files survive)
//! and whose first parent is the passed `parent`. The second parent is
//! `merge.upstream_commit` (a real edge into the `refs/vendor/<name>` graph) by
//! default, or — when `entry.mode` is `Squash` — a fresh parentless commit holding
//! only the remapped vendor tree, severing upstream reachability. It writes only
//! the commit object(s): no `HEAD` or branch ref is moved.
//!
//! The fixture mirrors the `merge_vendor` tests: a bare local repo with the
//! local ("ours") commit fetched in and an upstream commit fetched via
//! `fetch_vendor`, then a real merge produced to commit.

use git_vendor::{
    PatternMapping, VendorEntry, VendorMerge, VendorMode, VendorName, VendorRepository as _,
};
use gix::bstr::ByteSlice as _;

use crate::support::{git, git_capture, init, tree_entries, write};

fn blob_at(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Vec<u8> {
    let oid = tree_entries(repo, tree)
        .get(path)
        .copied()
        .unwrap_or_else(|| panic!("path {path:?} absent from tree"));
    repo.find_object(oid).expect("find blob").data.clone()
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
        mode: VendorMode::Merge,
    }
}

fn squash_entry(url: &str) -> VendorEntry {
    let mut e = entry(url);
    e.mode = VendorMode::Squash;
    e
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

/// The minted commit's tree is the vendor-only `merge.result_tree` spliced into
/// the full `parent` tree — non-vendored files (`README`, `.gitattributes`)
/// survive alongside the merged vendor content — and its parents are exactly
/// `[parent, merge.upstream_commit]` in that order: "ours" first, upstream second.
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

    let tree = decoded.tree();
    assert_ne!(
        tree, b.merge.result_tree,
        "merge commit tree must be the full repo tree, not the vendor-only result",
    );
    let paths: Vec<String> = tree_entries(&b.repo, tree).into_keys().collect();
    assert_eq!(
        paths,
        vec![
            ".gitattributes".to_owned(),
            "README".to_owned(),
            "vendor/keep.txt".to_owned(),
            "vendor/x.txt".to_owned(),
        ],
    );
    assert_eq!(blob_at(&b.repo, tree, "README"), b"scratch");
    assert_eq!(blob_at(&b.repo, tree, "vendor/keep.txt"), b"local\n");
    assert_eq!(blob_at(&b.repo, tree, "vendor/x.txt"), b"upstream\n");

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

/// In squash mode the merge commit still has two parents — `[ours, synthetic]` —
/// but the second parent is a fresh commit, *not* `merge.upstream_commit`.
#[test]
fn squash_second_parent_is_synthetic() {
    let b = build();
    let e = squash_entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig("C", "c@example.com", "1700000000 +0000"),
            sig("A", "a@example.com", "1700000000 +0000"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let commit = b.repo.find_commit(oid).expect("find_commit");
    let decoded = commit.decode().expect("decode");

    // The merge commit's tree is the full spliced tree regardless of mode; mode
    // only changes the second parent. (Tree content is covered in detail by
    // `mints_two_parent_merge_commit`.)
    assert_ne!(decoded.tree(), b.merge.result_tree);
    let parents: Vec<gix::ObjectId> = decoded.parents().collect();
    assert_eq!(parents.len(), 2);
    assert_eq!(parents[0], b.ours);
    assert_ne!(
        parents[1], b.merge.upstream_commit,
        "squash must not record the upstream commit directly",
    );
}

/// The synthetic second parent severs reachability to upstream history: it is
/// parentless, its tree is the remapped vendor tree (local path space), and it
/// records the squashed-from upstream OID in its message so that point is
/// recoverable from a clone lacking the upstream graph.
#[test]
fn squash_synthetic_commit_is_thin_and_records_upstream() {
    let b = build();
    let e = squash_entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig("C", "c@example.com", "1700000000 +0000"),
            sig("A", "a@example.com", "1700000000 +0000"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let merge = b.repo.find_commit(oid).expect("find_commit");
    let second_parent = merge.decode().expect("decode").parents().nth(1).unwrap();
    let squash = b
        .repo
        .find_commit(second_parent)
        .expect("find squash commit");
    let decoded = squash.decode().expect("decode");

    assert_eq!(
        decoded.parents().count(),
        0,
        "squash commit must be parentless",
    );
    assert_eq!(
        decoded.tree(),
        b.repo
            .upstream_tree(&e, b.merge.upstream_commit)
            .expect("upstream_tree"),
    );
    assert!(
        decoded
            .message
            .to_str_lossy()
            .contains(&b.merge.upstream_commit.to_string()),
        "message {:?} must record the upstream OID",
        decoded.message,
    );
}

/// The squashed-from upstream OID rides as a real git trailer
/// (`Squashed-upstream: <oid>`), so it is machine-readable — not merely present
/// somewhere in the free-text message.
#[test]
fn squash_records_upstream_as_trailer() {
    let b = build();
    let e = squash_entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig("C", "c@example.com", "1700000000 +0000"),
            sig("A", "a@example.com", "1700000000 +0000"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let merge = b.repo.find_commit(oid).expect("find_commit");
    let second_parent = merge.decode().expect("decode").parents().nth(1).unwrap();
    let squash = b.repo.find_commit(second_parent).expect("find squash");
    let decoded = squash.decode().expect("decode");

    let value = decoded
        .message_trailers()
        .find(|t| t.token == "Squashed-upstream".as_bytes().as_bstr())
        .map(|t| t.value.into_owned())
        .expect("Squashed-upstream trailer must be present");
    assert_eq!(
        value,
        b.merge.upstream_commit.to_string().as_bytes().as_bstr()
    );
}

/// The synthetic commit shares the merge commit's author and committer
/// identities (and timestamps) verbatim: the two objects record one operation.
#[test]
fn squash_commit_shares_merge_identity() {
    let b = build();
    let e = squash_entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig(
                "Committer Name",
                "committer@example.com",
                "1700000123 +0100",
            ),
            sig("Author Name", "author@example.com", "1699999999 -0500"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let merge = b.repo.find_commit(oid).expect("find_commit");
    let second_parent = merge.decode().expect("decode").parents().nth(1).unwrap();
    let squash = b.repo.find_commit(second_parent).expect("find squash");
    let decoded = squash.decode().expect("decode");

    let author = decoded.author().expect("author");
    let committer = decoded.committer().expect("committer");
    assert_eq!(author.name, "Author Name");
    assert_eq!(author.email, "author@example.com");
    assert_eq!(author.time, "1699999999 -0500");
    assert_eq!(committer.name, "Committer Name");
    assert_eq!(committer.email, "committer@example.com");
    assert_eq!(committer.time, "1700000123 +0100");
}

/// The synthetic commit's tree is exactly the remapped vendor tree: upstream
/// `up/x.txt` lands at `vendor/x.txt` with its bytes intact, the unmatched
/// upstream `README` is dropped, and nothing local-only leaks in.
#[test]
fn squash_synthetic_tree_holds_remapped_upstream_content() {
    let b = build();
    let e = squash_entry("unused");

    let oid = b
        .repo
        .commit_vendor(
            sig("C", "c@example.com", "1700000000 +0000"),
            sig("A", "a@example.com", "1700000000 +0000"),
            "merge mylib",
            &e,
            b.ours,
            &b.merge,
        )
        .expect("commit_vendor");

    let merge = b.repo.find_commit(oid).expect("find_commit");
    let second_parent = merge.decode().expect("decode").parents().nth(1).unwrap();
    let squash = b.repo.find_commit(second_parent).expect("find squash");
    let tree = squash.decode().expect("decode").tree();

    let paths: Vec<String> = tree_entries(&b.repo, tree).into_keys().collect();
    assert_eq!(paths, vec!["vendor/x.txt".to_owned()]);
    assert_eq!(blob_at(&b.repo, tree, "vendor/x.txt"), b"upstream\n");
}

/// Squash mode is likewise a pure function of its arguments.
#[test]
fn squash_is_deterministic() {
    let b = build();
    let e = squash_entry("unused");

    let commit = |b: &Built| {
        b.repo
            .commit_vendor(
                sig("C", "c@example.com", "1700000000 +0000"),
                sig("A", "a@example.com", "1700000000 +0000"),
                "merge",
                &e,
                b.ours,
                &b.merge,
            )
            .expect("commit_vendor")
    };

    assert_eq!(commit(&b), commit(&b));
}
