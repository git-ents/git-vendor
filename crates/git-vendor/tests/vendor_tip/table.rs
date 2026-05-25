//! Table-driven and integration tests for `vendor_tip`.

use std::path::Path;

use git_vendor::{PatternMapping, VendorEntry, VendorMode, VendorName, VendorRepository as _};
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

/// Like [`make_upstream`] but with caller-chosen file content, so two upstreams
/// have distinct tree (and therefore commit) OIDs regardless of timing.
fn make_upstream_with(dir: &Path, content: &[u8]) -> gix::ObjectId {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join("hello.txt"), content).unwrap();
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

fn make_entry(
    name: &str,
    url: &str,
    ref_name: Option<&str>,
    patterns: Vec<PatternMapping>,
) -> VendorEntry {
    VendorEntry {
        name: VendorName::new(name).unwrap(),
        url: url.to_owned(),
        ref_name: ref_name.map(str::to_owned),
        base: None,
        patterns,
        mode: VendorMode::Merge,
    }
}

// ── Unfetched ─────────────────────────────────────────────────────────────────

/// `vendor_tip` is namespaced per vendor: fetching one vendor leaves an
/// unrelated vendor's tip absent.
#[test]
fn tip_isolated_per_vendor_name() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let url = upstream.path().to_str().unwrap();

    let fetched = make_entry("alpha", url, None, vec![]);
    repo.fetch_vendor(&fetched).expect("fetch_vendor");

    let other = make_entry("beta", url, None, vec![]);
    assert!(
        repo.vendor_tip(&other).expect("vendor_tip").is_none(),
        "an unfetched vendor must not see another vendor's tip",
    );
}

/// Two vendors fetched from upstreams with distinct content keep distinct
/// tips, so `vendor_tip` never aliases two names onto one ref. The isolation
/// test above only proves that nothing leaks into an unrelated, never-fetched
/// name; a genuine name collision would still pass it. Fetching both vendors
/// and asserting that each tip is its own closes that gap.
#[test]
fn tip_distinct_per_vendor_from_separate_upstreams() {
    let up_a = tempfile::tempdir().unwrap();
    let up_b = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let tip_a = make_upstream_with(up_a.path(), b"alpha");
    let tip_b = make_upstream_with(up_b.path(), b"beta");
    assert_ne!(
        tip_a, tip_b,
        "upstreams must differ for the test to mean anything"
    );

    let repo = make_local(local.path());
    let entry_a = make_entry("alpha", up_a.path().to_str().unwrap(), None, vec![]);
    let entry_b = make_entry("beta", up_b.path().to_str().unwrap(), None, vec![]);

    repo.fetch_vendor(&entry_a).expect("fetch alpha");
    repo.fetch_vendor(&entry_b).expect("fetch beta");

    assert_eq!(repo.vendor_tip(&entry_a).expect("tip alpha"), Some(tip_a));
    assert_eq!(repo.vendor_tip(&entry_b).expect("tip beta"), Some(tip_b));
}

// ── Consistency with fetch_vendor ─────────────────────────────────────────────

/// After `fetch_vendor`, `vendor_tip` returns exactly the OID `fetch_vendor`
/// returned, for the default tracking ref and an explicit branch alike.
#[rstest]
#[case(None)]
#[case(Some("main"))]
fn tip_matches_fetch_return(#[case] ref_name: Option<&str>) {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), ref_name, vec![]);

    let fetched = repo.fetch_vendor(&entry).expect("fetch_vendor");
    let tip = repo.vendor_tip(&entry).expect("vendor_tip");
    assert_eq!(tip, Some(fetched));
}

/// For an annotated tag, `vendor_tip` returns the peeled target — the same id
/// `fetch_vendor` returns — even though the ref stores the tag object itself.
#[test]
fn tip_peels_annotated_tag_like_fetch() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let commit = make_upstream(upstream.path());
    git(&["tag", "-a", "v1", "-m", "release one"], upstream.path());
    let tag_obj = rev_parse(upstream.path(), "v1");
    assert_ne!(tag_obj, commit, "annotated tag must be its own object");

    let repo = make_local(local.path());
    let entry = make_entry(
        "mylib",
        upstream.path().to_str().unwrap(),
        Some("v1"),
        vec![],
    );

    let fetched = repo.fetch_vendor(&entry).expect("fetch_vendor");
    let tip = repo.vendor_tip(&entry).expect("vendor_tip");

    assert_eq!(tip, Some(commit), "tip must be the peeled target");
    assert_eq!(tip, Some(fetched), "tip must agree with fetch_vendor");
}

/// For a lightweight tag (a ref pointing straight at the commit, no tag
/// object), `vendor_tip` returns that commit and agrees with `fetch_vendor`.
/// Peeling is a no-op here, so this guards the boundary opposite the annotated
/// case above.
#[test]
fn tip_resolves_lightweight_tag() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let commit = make_upstream(upstream.path());
    git(&["tag", "v1"], upstream.path());
    assert_eq!(
        rev_parse(upstream.path(), "v1"),
        commit,
        "lightweight tag must resolve straight to the commit",
    );

    let repo = make_local(local.path());
    let entry = make_entry(
        "mylib",
        upstream.path().to_str().unwrap(),
        Some("v1"),
        vec![],
    );

    let fetched = repo.fetch_vendor(&entry).expect("fetch_vendor");
    let tip = repo.vendor_tip(&entry).expect("vendor_tip");

    assert_eq!(tip, Some(commit));
    assert_eq!(tip, Some(fetched), "tip must agree with fetch_vendor");
}

/// A second fetch after a non-fast-forward upstream rewrite moves the tip;
/// `vendor_tip` reflects the new value, not the stale one.
#[test]
fn tip_reflects_refetch_after_rewrite() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let first_tip = make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry(
        "mylib",
        upstream.path().to_str().unwrap(),
        Some("main"),
        vec![],
    );

    repo.fetch_vendor(&entry).expect("first fetch");
    assert_eq!(repo.vendor_tip(&entry).expect("tip"), Some(first_tip));

    git(&["commit", "--amend", "-m", "rewritten"], upstream.path());
    let rewritten = rev_parse(upstream.path(), "HEAD");
    assert_ne!(rewritten, first_tip);

    repo.fetch_vendor(&entry).expect("second fetch");
    assert_eq!(repo.vendor_tip(&entry).expect("tip"), Some(rewritten));
}

// ── Reads the local ref only ──────────────────────────────────────────────────

/// `vendor_tip` resolves only `entry.vendor_ref()`: it ignores `url`,
/// `ref_name`, and `patterns`, and never touches the network. After a fetch,
/// querying with a different entry that shares only the vendor name — but
/// carries a bogus URL, a different tracking ref, and patterns — still returns
/// the cached tip.
#[test]
fn tip_depends_only_on_vendor_ref() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());

    let fetched = make_entry(
        "mylib",
        upstream.path().to_str().unwrap(),
        Some("main"),
        vec![],
    );
    let tip = repo.fetch_vendor(&fetched).expect("fetch_vendor");

    let probe = make_entry(
        "mylib",
        "https://this-host-does-not-resolve.invalid/repo.git",
        Some("some-other-ref"),
        vec![PatternMapping {
            glob: "src/**".to_owned(),
            destination: Some("third_party/".to_owned()),
        }],
    );

    assert_eq!(
        repo.vendor_tip(&probe).expect("vendor_tip"),
        Some(tip),
        "tip must come from the local ref, independent of url/ref/patterns",
    );
}

// ── Corrupt ref ───────────────────────────────────────────────────────────────

/// A `refs/vendor/<name>` that exists but points at an absent object (a
/// dangling ref — e.g. its objects were gc'd) is an error, not `None` and not
/// a panic: `vendor_tip` finds the ref but `peel_to_id` fails. This pins the
/// only reachable `Err` arm of `vendor_tip`.
#[test]
fn tip_errors_on_dangling_ref() {
    let local = tempfile::tempdir().unwrap();
    let repo = make_local(local.path());

    let ref_path = local.path().join("refs").join("vendor").join("mylib");
    std::fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
    // Well-formed SHA-1 hex for an object that does not exist.
    std::fs::write(&ref_path, format!("{}\n", "1".repeat(40))).unwrap();

    let entry = make_entry("mylib", "https://example.com/repo.git", None, vec![]);
    assert!(
        repo.vendor_tip(&entry).is_err(),
        "a ref to a missing object must surface as an error",
    );
}
