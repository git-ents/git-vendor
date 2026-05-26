//! Table-driven and integration tests for `vendor_status`.
//!
//! `vendor_status` peels `refs/vendor/<name>` to the current upstream tip and
//! compares it against `entry.base` with a merge-base test: equal ids are
//! `UpToDate`, a tip that descends from `base` is an `UpdateAvailable`, and a
//! tip that does not (divergent, unrelated, or rewound history) is
//! `ForcePushed`. A missing ref is `NotFetched`; a missing `base` is the first
//! `UpdateAvailable`.

use git_vendor::{VendorEntry, VendorMode, VendorName, VendorRepository as _, VendorStatus};

use crate::support::{git, init, make_local, make_upstream, push_commit};

fn make_entry(name: &str, url: &str, base: Option<gix::ObjectId>) -> VendorEntry {
    VendorEntry {
        name: VendorName::new(name).unwrap(),
        url: url.to_owned(),
        ref_name: Some("main".to_owned()),
        base,
        patterns: vec![],
        mode: VendorMode::Merge,
    }
}

/// The upstream OID carried by the variants that have one.
fn upstream_of(status: &VendorStatus) -> Option<gix::ObjectId> {
    match status {
        VendorStatus::UpdateAvailable { upstream } | VendorStatus::ForcePushed { upstream } => {
            Some(*upstream)
        }
        VendorStatus::NotFetched | VendorStatus::UpToDate => None,
    }
}

// ── NotFetched ──────────────────────────────────────────────────────────────

/// With no `refs/vendor/<name>`, the vendor is `NotFetched` regardless of
/// whether a `base` is recorded — the missing ref short-circuits before `base`
/// is consulted.
#[test]
fn missing_ref_is_not_fetched() {
    let local = tempfile::tempdir().unwrap();
    let repo = make_local(local.path());

    let entry = make_entry("mylib", "https://example.com/repo.git", None);
    assert!(matches!(
        repo.vendor_status(&entry).expect("status"),
        VendorStatus::NotFetched,
    ));

    // A recorded base must not conjure a status out of an absent ref.
    let bogus = gix::ObjectId::from_hex(&[b'1'; 40]).unwrap();
    let entry = make_entry("mylib", "https://example.com/repo.git", Some(bogus));
    assert!(matches!(
        repo.vendor_status(&entry).expect("status"),
        VendorStatus::NotFetched,
    ));
}

// ── First update (no base) ────────────────────────────────────────────────────

/// Fetched but never merged (`base: None`): the tip is the first update to
/// apply, reported as `UpdateAvailable` carrying that tip.
#[test]
fn fetched_without_base_is_update_available() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), None);

    let tip = repo.fetch_vendor(&entry).expect("fetch_vendor");
    let status = repo.vendor_status(&entry).expect("status");

    assert!(matches!(status, VendorStatus::UpdateAvailable { .. }));
    assert_eq!(upstream_of(&status), Some(tip));
}

// ── UpToDate ────────────────────────────────────────────────────────────────

/// When the recorded base equals the current tip, the vendor is `UpToDate`.
#[test]
fn base_equal_to_tip_is_up_to_date() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    let repo = make_local(local.path());
    let mut entry = make_entry("mylib", upstream.path().to_str().unwrap(), None);

    let tip = repo.fetch_vendor(&entry).expect("fetch_vendor");
    entry.base = Some(tip);

    assert!(matches!(
        repo.vendor_status(&entry).expect("status"),
        VendorStatus::UpToDate,
    ));
}

// ── UpdateAvailable (fast-forward) ────────────────────────────────────────────

/// Base is the parent of the tip: a one-commit fast-forward is `UpdateAvailable`
/// carrying the new tip.
#[test]
fn base_ancestor_one_commit_is_update_available() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let base = make_upstream(upstream.path());
    let repo = make_local(local.path());
    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), None);

    repo.fetch_vendor(&entry).expect("first fetch");
    let tip = push_commit(upstream.path(), b"v2");
    repo.fetch_vendor(&entry).expect("refetch");
    assert_ne!(base, tip);

    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), Some(base));
    let status = repo.vendor_status(&entry).expect("status");
    assert!(matches!(status, VendorStatus::UpdateAvailable { .. }));
    assert_eq!(upstream_of(&status), Some(tip));
}

// ── ForcePushed ─────────────────────────────────────────────────────────────

/// Divergent history sharing a common ancestor: the upstream branch was reset
/// below the recorded base and re-committed, so the new tip does not descend
/// from `base`. Reported as `ForcePushed`.
#[test]
fn divergent_history_is_force_pushed() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let root = make_upstream(upstream.path());
    let base = push_commit(upstream.path(), b"v2");
    let repo = make_local(local.path());
    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), Some(base));
    repo.fetch_vendor(&entry).expect("first fetch");

    // Rewind to the root and build a different commit: base is no longer
    // reachable from the new tip, but they still share `root`.
    git(&["reset", "--hard", &root.to_string()], upstream.path());
    let tip = push_commit(upstream.path(), b"other");
    repo.fetch_vendor(&entry).expect("refetch");
    assert_ne!(base, tip);

    let status = repo.vendor_status(&entry).expect("status");
    assert!(matches!(status, VendorStatus::ForcePushed { .. }));
    assert_eq!(upstream_of(&status), Some(tip));
}

/// The tip is an *ancestor* of the recorded base (upstream rewound to an older
/// commit). The base no longer descends from the tip, so this is `ForcePushed`,
/// not `UpToDate`.
#[test]
fn rewound_tip_is_force_pushed() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    let old = make_upstream(upstream.path());
    let base = push_commit(upstream.path(), b"v2");
    let repo = make_local(local.path());
    let entry = make_entry("mylib", upstream.path().to_str().unwrap(), Some(base));
    repo.fetch_vendor(&entry).expect("first fetch");

    git(&["reset", "--hard", &old.to_string()], upstream.path());
    repo.fetch_vendor(&entry).expect("refetch");

    let status = repo.vendor_status(&entry).expect("status");
    assert!(matches!(status, VendorStatus::ForcePushed { .. }));
    assert_eq!(upstream_of(&status), Some(old));
}

/// Unrelated histories with no merge base at all: the recorded base comes from
/// a different root than the tip. The `merge_base` lookup returns `NotFound`,
/// which the implementation treats as `ForcePushed` rather than an error.
#[test]
fn unrelated_history_is_force_pushed() {
    let up_main = tempfile::tempdir().unwrap();
    let up_other = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(up_main.path());
    // Distinct content so this root's OID cannot collide with up_main's
    // identical-content root and accidentally read as the same history.
    init(up_other.path());
    let other_root = push_commit(up_other.path(), b"unrelated");
    let repo = make_local(local.path());

    // Fetch the unrelated repo under its own vendor name so its object lands in
    // the local db, then use that OID as our vendor's base.
    let other = make_entry("other", up_other.path().to_str().unwrap(), None);
    repo.fetch_vendor(&other).expect("fetch other");

    let entry = make_entry("mylib", up_main.path().to_str().unwrap(), None);
    let tip = repo.fetch_vendor(&entry).expect("fetch mylib");

    let entry = make_entry("mylib", up_main.path().to_str().unwrap(), Some(other_root));
    let status = repo.vendor_status(&entry).expect("status");
    assert!(matches!(status, VendorStatus::ForcePushed { .. }));
    assert_eq!(upstream_of(&status), Some(tip));
}

// ── Corrupt ref ─────────────────────────────────────────────────────────────

/// A `refs/vendor/<name>` pointing at an absent object errors during the peel,
/// before `base` is consulted — surfaced as `Err`, not a panic. Pins the only
/// reachable error arm of `vendor_status`.
#[test]
fn dangling_ref_errors() {
    let local = tempfile::tempdir().unwrap();
    let repo = make_local(local.path());

    let ref_path = local.path().join("refs").join("vendor").join("mylib");
    std::fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
    std::fs::write(&ref_path, format!("{}\n", "1".repeat(40))).unwrap();

    let entry = make_entry("mylib", "https://example.com/repo.git", None);
    assert!(repo.vendor_status(&entry).is_err());
}
