//! Table-driven tests for `fetch_vendor`.

use std::path::Path;

use git_vendor::{Error, PatternMapping, VendorEntry, VendorName, VendorRepository as _};
use rstest::rstest;

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

/// Run `git rev-parse <rev>` in `dir` and return the resolved OID.
fn rev_parse(dir: &Path, rev: &str) -> gix::ObjectId {
    let out = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("rev-parse");
    assert!(out.status.success(), "rev-parse {rev} failed in {dir:?}");
    let hex = std::str::from_utf8(&out.stdout).unwrap().trim();
    gix::ObjectId::from_hex(hex.as_bytes()).expect("valid hex")
}

/// Initialize a git repo at `dir` with one commit on `main`, returning the HEAD OID.
fn make_upstream(dir: &Path) -> gix::ObjectId {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join("hello.txt"), b"hello").unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-m", "init"], dir);
    rev_parse(dir, "HEAD")
}

/// Initialize a bare local repo that will fetch from upstream.
fn make_local(dir: &Path) -> gix::Repository {
    git(&["init", "--bare", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    gix::open(dir).expect("gix open")
}

fn make_entry(url: &str, ref_name: Option<&str>, patterns: Vec<PatternMapping>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: url.to_owned(),
        ref_name: ref_name.map(str::to_owned),
        base: None,
        patterns,
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
