//! Property-based tests for invariants of `vendor_tip`.
//!
//! `vendor_tip` consults only `entry.vendor_ref()`, so the only inputs that
//! matter are the vendor name, which forms the ref path, and whether that ref
//! exists. The `ref_name`, `base`, and `patterns` fields never reach a code
//! path here, so varying them would assert the same thing on every case
//! without adding coverage. The properties instead fuzz the name across the
//! find-and-peel path.

use std::path::Path;
use std::process::Command;

use git_vendor::{VendorEntry, VendorName, VendorRepository as _};
use proptest::prelude::*;

// ── Fixture helpers ───────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let output = Command::new("git")
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

/// A repo with one commit, returning its gix handle and the commit OID.
fn repo_with_commit(dir: &Path) -> (gix::Repository, gix::ObjectId) {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join("hello.txt"), b"hello").unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-m", "init"], dir);
    let repo = gix::open(dir).expect("gix open");
    let id = repo.head_id().expect("head id").detach();
    (repo, id)
}

fn entry(name: VendorName) -> VendorEntry {
    VendorEntry {
        name,
        url: "https://example.com/repo.git".to_owned(),
        ref_name: None,
        base: None,
        patterns: vec![],
    }
}

// ── Strategies ────────────────────────────────────────────────────────────────

fn name_strategy() -> impl Strategy<Value = VendorName> {
    "[a-z][a-z0-9-]{0,15}".prop_map(|s| VendorName::new(s).expect("strategy yields valid names"))
}

// ── Invariants ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// On a repository where `refs/vendor/<name>` was never created,
    /// `vendor_tip` is `Ok(None)` for any valid name: a never-fetched vendor is
    /// reported as absent, never as an error and never with a panic.
    #[test]
    fn unfetched_tip_is_none(name in name_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).expect("init bare");
        prop_assert_eq!(repo.vendor_tip(&entry(name)).expect("vendor_tip"), None);
    }
}

proptest! {
    // Each case spawns git to build a real commit, so keep the count modest.
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// For any valid name, a `refs/vendor/<name>` pointing at a known commit
    /// round-trips: `vendor_tip` resolves *exactly* that commit. This fuzzes
    /// the find-and-peel path and would catch a name that escapes or is
    /// misquoted into the ref path.
    #[test]
    fn tip_round_trips_for_any_valid_name(name in name_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let (repo, commit) = repo_with_commit(dir.path());
        git(
            &["update-ref", &format!("refs/vendor/{name}"), &commit.to_string()],
            dir.path(),
        );
        prop_assert_eq!(
            repo.vendor_tip(&entry(name)).expect("vendor_tip"),
            Some(commit),
        );
    }
}
