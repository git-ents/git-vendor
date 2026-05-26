//! Table-driven tests for `fetch_vendor`.

use git_vendor::{
    Error, PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _,
};
use rstest::rstest;

use crate::support::{git, make_local, make_upstream, rev_parse};

fn make_entry(url: &str, ref_name: Option<&str>, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: ref_name.map(str::to_owned),
        base: None,
        patterns,
        mode: VendorMode::Merge,
    }
}

// ── tracking_ref ──────────────────────────────────────────────────────────────

/// `tracking_ref()` is `HEAD` when unset, otherwise the configured ref.
#[rstest]
#[case(None, "HEAD")]
#[case(Some("main"), "main")]
#[case(Some("v1.2.3"), "v1.2.3")]
fn tracking_ref_resolves(#[case] ref_name: Option<&str>, #[case] expected: &str) {
    let entry = make_entry("https://example.com/repo.git", ref_name, vec![]);
    assert_eq!(entry.tracking_ref(), expected);
}

// ── Happy path ────────────────────────────────────────────────────────────────

/// Fetch with the default tracking ref (HEAD) and an explicit branch both return
/// the upstream tip OID.
#[rstest]
#[case(None)]
#[case(Some("main"))]
fn fetch_returns_upstream_tip(#[case] ref_name: Option<&str>) {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let expected = make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), ref_name, vec![]);

    let got = repo.fetch_vendor(&entry).expect("fetch_vendor");
    assert_eq!(got, expected);
}

/// After `fetch_vendor`, `refs/vendor/<name>` resolves to the returned OID.
#[test]
fn fetch_updates_vendor_ref() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), None, vec![]);

    let fetched_oid = repo.fetch_vendor(&entry).expect("fetch_vendor");

    let mut reference = repo.find_reference(&entry.vendor_ref()).expect("find ref");
    let ref_oid = reference.peel_to_id().expect("peel").detach();

    assert_eq!(fetched_oid, ref_oid);
}

/// Calling `fetch_vendor` twice without any upstream changes returns the same OID.
#[test]
fn fetch_is_stable() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), None, vec![]);

    let first = repo.fetch_vendor(&entry).expect("first fetch");
    let second = repo.fetch_vendor(&entry).expect("second fetch");
    assert_eq!(first, second);
}

/// A second fetch after the upstream history is rewritten (a non-fast-forward
/// amend) force-updates `refs/vendor/<name>` to the new tip. This exercises the
/// leading `+` in the refspec.
#[test]
fn fetch_force_updates_on_upstream_rewrite() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let first_tip = make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), Some("main"), vec![]);

    let first = repo.fetch_vendor(&entry).expect("first fetch");
    assert_eq!(first, first_tip);

    git(&["commit", "--amend", "-m", "rewritten"], upstream.path());
    let rewritten_tip = rev_parse(upstream.path(), "HEAD");
    assert_ne!(rewritten_tip, first_tip);

    let second = repo.fetch_vendor(&entry).expect("second fetch");
    assert_eq!(second, rewritten_tip);

    let mut reference = repo.find_reference(&entry.vendor_ref()).expect("find ref");
    assert_eq!(
        reference.peel_to_id().expect("peel").detach(),
        rewritten_tip
    );
}

/// Fetching an annotated tag stores the tag object at `refs/vendor/<name>` but
/// returns the tag's ultimate (peeled) target, per the documented contract.
#[test]
fn fetch_peels_annotated_tag() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let commit = make_upstream(upstream.path());
    git(&["tag", "-a", "v1", "-m", "release one"], upstream.path());
    let tag_obj = rev_parse(upstream.path(), "v1");
    assert_ne!(tag_obj, commit, "annotated tag must be its own object");

    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), Some("v1"), vec![]);

    let got = repo.fetch_vendor(&entry).expect("fetch_vendor");
    assert_eq!(got, commit, "returned id must be the peeled target");

    let reference = repo.find_reference(&entry.vendor_ref()).expect("find ref");
    assert_eq!(
        reference.id().detach(),
        tag_obj,
        "stored ref must point at the tag object itself",
    );
}

/// `fetch_vendor` does not pull upstream tags into the local repo; only the
/// configured tracking ref is fetched.
#[test]
fn fetch_does_not_pull_tags() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    git(&["tag", "-a", "v1", "-m", "release one"], upstream.path());

    let repo = make_local(local.path());
    let entry = make_entry(upstream.path().to_str().unwrap(), None, vec![]);

    repo.fetch_vendor(&entry).expect("fetch_vendor");

    assert!(
        repo.find_reference("refs/tags/v1").is_err(),
        "vendor fetch must not import upstream tags",
    );
}

/// A vendor entry with patterns present fetches the same tip as one without.
///
/// Patterns only affect tree filtering, not what `fetch_vendor` fetches.
#[test]
fn fetch_ignores_patterns() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let expected = make_upstream(upstream.path());
    let repo = make_local(local.path());

    let patterns = vec![PatternMapping {
        glob: "src/**".to_owned(),
        destination: Some("third_party/".to_owned()),
    }];
    let entry = make_entry(upstream.path().to_str().unwrap(), None, patterns);

    let got = repo.fetch_vendor(&entry).expect("fetch_vendor");
    assert_eq!(got, expected);
}

// ── Error paths ───────────────────────────────────────────────────────────────

/// Unrecognized URL schemes are rejected up front by the scheme guard, before
/// any network or process activity.
#[rstest]
#[case("unknownscheme://example.com/repo.git")]
#[case("ftp://example.com/repo.git")]
fn rejects_unknown_url_scheme(#[case] url: &str) {
    let local = tempfile::tempdir().unwrap();
    let repo = make_local(local.path());
    let entry = make_entry(url, None, vec![]);

    match repo.fetch_vendor(&entry) {
        Err(Error::InvalidUrl(_)) => {}
        other => panic!("expected Error::InvalidUrl, got {other:?}"),
    }
}

/// Fetching an `ext::` URL must never execute the embedded command. gix does
/// not implement the `ext::` transport, so this currently fails at connect
/// time rather than via the scheme guard; either way, the security-relevant
/// property is that the command does not run.
#[test]
fn ext_url_does_not_execute_command() {
    let local = tempfile::tempdir().unwrap();
    let repo = make_local(local.path());
    let marker = local.path().join("pwned");
    let url = format!("ext::touch {}", marker.display());
    let entry = make_entry(&url, None, vec![]);

    assert!(repo.fetch_vendor(&entry).is_err());
    assert!(!marker.exists(), "ext:: command must not be executed");
}

/// A tracking ref that does not exist upstream is reported as `Error::Fetch`
/// rather than silently succeeding or panicking on the missing local ref.
#[test]
fn fetch_errors_when_ref_missing() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(
        upstream.path().to_str().unwrap(),
        Some("does-not-exist"),
        vec![],
    );

    match repo.fetch_vendor(&entry) {
        Err(Error::Fetch(_)) => {}
        other => panic!("expected Error::Fetch, got {other:?}"),
    }
}

// ── Regression: fetch into a non-bare local on the same default branch ────────

/// Bug demo. The contract of `fetch_vendor` is "fetch the upstream tracking
/// ref and return its *upstream* tip" — the OID identifies an upstream commit
/// whose objects are now in the local odb. The refspec is
/// `+HEAD:refs/vendor/<name>` against the upstream URL.
///
/// When the local repo is *non-bare* and is itself checked out on a branch of
/// the same name as the upstream default (`main`), the value returned —
/// which is what `refs/vendor/<name>` ends up pointing at — is the *local*
/// HEAD, not the upstream HEAD. The upstream's commit and its objects are
/// not brought into the local odb at all, so a follow-on `upstream_tree`,
/// `base_tree`, or `merge_vendor` keyed on the returned OID operates on a
/// completely different tree than intended (silently empty in the example
/// here, since the local HEAD's tree has no `up/` directory).
///
/// The bare-local fetch path is exercised throughout the rest of this file
/// and is unaffected; this test pins the non-bare case so the regression is
/// hard to reintroduce once fixed.
///
/// Marked `#[should_panic]` so CI passes while the upstream gix bug stands;
/// when the bug is fixed this flips to a regular failure and the attribute
/// is removed.
#[test]
#[should_panic]
fn fetch_returns_upstream_tip_into_non_bare_local() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    // Upstream HEAD has a unique blob at `up/marker.txt` — an oracle for
    // "did we really pick up the upstream commit and its objects?".
    git(&["init", "-b", "main"], upstream.path());
    git(
        &["config", "user.email", "test@example.com"],
        upstream.path(),
    );
    git(&["config", "user.name", "Test"], upstream.path());
    std::fs::create_dir_all(upstream.path().join("up")).unwrap();
    std::fs::write(upstream.path().join("up/marker.txt"), b"upstream-marker").unwrap();
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "upstream"], upstream.path());
    let upstream_head = rev_parse(upstream.path(), "HEAD");

    // Local: non-bare, on `main`, with its own commit that does not contain
    // `up/`. This is the configuration that triggers the bug.
    git(&["init", "-b", "main"], local.path());
    git(&["config", "user.email", "test@example.com"], local.path());
    git(&["config", "user.name", "Test"], local.path());
    std::fs::write(local.path().join("local-only.txt"), b"local").unwrap();
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "local"], local.path());
    let local_head = rev_parse(local.path(), "HEAD");
    assert_ne!(
        local_head, upstream_head,
        "fixture invariant: heads must differ so the oracle is meaningful",
    );

    let repo = gix::open(local.path()).expect("gix open");
    let entry = make_entry(upstream.path().to_str().unwrap(), None, vec![]);

    let got = repo.fetch_vendor(&entry).expect("fetch_vendor");

    // 1. The returned OID must be the upstream HEAD, never the local HEAD.
    assert_eq!(
        got, upstream_head,
        "fetch_vendor returned the local HEAD {local_head} instead of the \
         upstream HEAD {upstream_head}; refs/vendor/<name> now points into \
         the local history, not into the fetched upstream",
    );
    // 2. The returned commit's objects must actually be in the local odb and
    //    carry the upstream content — even if (1) held by luck, this would
    //    catch a return that points to an absent or stale tree.
    let tree = repo
        .find_commit(got)
        .expect("commit object in local odb")
        .tree()
        .expect("tree");
    let has_marker = tree
        .traverse()
        .breadthfirst
        .files()
        .expect("traverse")
        .into_iter()
        .any(|r| r.filepath == "up/marker.txt");
    assert!(
        has_marker,
        "fetched tree lacks the upstream-only `up/marker.txt`: upstream \
         objects were not actually brought into the local odb",
    );
}
