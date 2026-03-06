//! Unit tests for `git-vendor`.

use git2::Config;
use std::io::Write;
use tempfile::{NamedTempFile, TempPath};

use super::VendorSource;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write an ini-style git config into a temp file and return both so the
/// caller can keep the `NamedTempFile` alive for as long as the `Config` is
/// needed (dropping the file closes/deletes it, which would invalidate the
/// config handle on some platforms).
fn config_from_str(s: &str) -> (TempPath, Config) {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(s.as_bytes()).unwrap();
    // Close the open file handle before handing the path to libgit2.
    // On Windows, libgit2 writes config changes via a lockfile rename, which
    // fails with "Access is denied" when the underlying file is still open.
    let path = tmp.into_temp_path();
    let cfg = Config::open(&path).unwrap();
    (path, cfg)
}

/// Return a writable `Config` backed by an empty temp file.
fn empty_config() -> (TempPath, Config) {
    config_from_str("")
}

// ---------------------------------------------------------------------------
// VendorSource::head_ref
// ---------------------------------------------------------------------------

#[test]
fn test_head_ref_simple() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
    };
    assert_eq!(vs.head_ref(), "refs/vendor/foo");
}

#[test]
fn test_head_ref_with_hyphens_and_underscores() {
    let vs = VendorSource {
        name: "my-cool_lib".into(),
        url: "https://example.com/lib.git".into(),
        branch: None,
        base: None,
    };
    assert_eq!(vs.head_ref(), "refs/vendor/my-cool_lib");
}

// ---------------------------------------------------------------------------
// VendorSource::tracking_branch
// ---------------------------------------------------------------------------

#[test]
fn test_tracking_branch_defaults_to_head() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
    };
    assert_eq!(vs.tracking_branch(), "HEAD");
}

#[test]
fn test_tracking_branch_uses_explicit_branch() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: Some("main".into()),
        base: None,
    };
    assert_eq!(vs.tracking_branch(), "main");
}

#[test]
fn test_tracking_branch_arbitrary_name() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: Some("release/v2".into()),
        base: None,
    };
    assert_eq!(vs.tracking_branch(), "release/v2");
}

// ---------------------------------------------------------------------------
// VendorSource::to_config
// ---------------------------------------------------------------------------

#[test]
fn test_to_config_url_only() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.foo.url").unwrap(),
        "https://example.com/foo.git"
    );
    // Optional fields must be absent.
    assert!(cfg.get_string("vendor.foo.branch").is_err());
    assert!(cfg.get_string("vendor.foo.base").is_err());
}

#[test]
fn test_to_config_with_branch() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "bar".into(),
        url: "https://example.com/bar.git".into(),
        branch: Some("develop".into()),
        base: None,
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.bar.url").unwrap(),
        "https://example.com/bar.git"
    );
    assert_eq!(cfg.get_string("vendor.bar.branch").unwrap(), "develop");
    assert!(cfg.get_string("vendor.bar.base").is_err());
}

#[test]
fn test_to_config_with_base() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "baz".into(),
        url: "https://example.com/baz.git".into(),
        branch: None,
        base: Some("cafebabe".into()),
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.baz.url").unwrap(),
        "https://example.com/baz.git"
    );
    assert!(cfg.get_string("vendor.baz.branch").is_err());
    assert_eq!(cfg.get_string("vendor.baz.base").unwrap(), "cafebabe");
}

#[test]
fn test_to_config_all_fields() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "full".into(),
        url: "https://example.com/full.git".into(),
        branch: Some("stable".into()),
        base: Some("deadbeef".into()),
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.full.url").unwrap(),
        "https://example.com/full.git"
    );
    assert_eq!(cfg.get_string("vendor.full.branch").unwrap(), "stable");
    assert_eq!(cfg.get_string("vendor.full.base").unwrap(), "deadbeef");
}

// ---------------------------------------------------------------------------
// VendorSource::from_config
// ---------------------------------------------------------------------------

#[test]
fn test_from_config_returns_none_for_unknown_vendor() {
    let (_tmp, cfg) = config_from_str("");
    let result = VendorSource::from_config(&cfg, "ghost").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_from_config_minimal() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "solo"]
    url = https://example.com/solo.git
"#,
    );
    let vs = VendorSource::from_config(&cfg, "solo").unwrap().unwrap();

    assert_eq!(vs.name, "solo");
    assert_eq!(vs.url, "https://example.com/solo.git");
    assert!(vs.branch.is_none());
    assert!(vs.base.is_none());
}

#[test]
fn test_from_config_with_branch() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "alpha"]
    url = https://example.com/alpha.git
    branch = next
"#,
    );
    let vs = VendorSource::from_config(&cfg, "alpha").unwrap().unwrap();

    assert_eq!(vs.name, "alpha");
    assert_eq!(vs.url, "https://example.com/alpha.git");
    assert_eq!(vs.branch.as_deref(), Some("next"));
    assert!(vs.base.is_none());
}

#[test]
fn test_from_config_with_base() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "beta"]
    url = https://example.com/beta.git
    base = 1a2b3c4d
"#,
    );
    let vs = VendorSource::from_config(&cfg, "beta").unwrap().unwrap();

    assert_eq!(vs.name, "beta");
    assert_eq!(vs.url, "https://example.com/beta.git");
    assert!(vs.branch.is_none());
    assert_eq!(vs.base.as_deref(), Some("1a2b3c4d"));
}

#[test]
fn test_from_config_all_fields() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "gamma"]
    url = https://example.com/gamma.git
    branch = release
    base = 0000ffff
"#,
    );
    let vs = VendorSource::from_config(&cfg, "gamma").unwrap().unwrap();

    assert_eq!(vs.name, "gamma");
    assert_eq!(vs.url, "https://example.com/gamma.git");
    assert_eq!(vs.branch.as_deref(), Some("release"));
    assert_eq!(vs.base.as_deref(), Some("0000ffff"));
}

#[test]
fn test_from_config_does_not_match_other_vendors() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "real"]
    url = https://example.com/real.git
"#,
    );
    // A different name must not be confused with "real".
    let result = VendorSource::from_config(&cfg, "real2").unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// VendorSource to_config / from_config roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_config_roundtrip_full() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "roundtrip".into(),
        url: "https://example.com/roundtrip.git".into(),
        branch: Some("main".into()),
        base: Some("abc123def456".into()),
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "roundtrip")
        .unwrap()
        .unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert_eq!(restored.branch, original.branch);
    assert_eq!(restored.base, original.base);
}

#[test]
fn test_config_roundtrip_optional_fields_absent() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "minimal".into(),
        url: "https://example.com/minimal.git".into(),
        branch: None,
        base: None,
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "minimal").unwrap().unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert!(restored.branch.is_none());
    assert!(restored.base.is_none());
}

// ---------------------------------------------------------------------------
// vendors_from_config
// ---------------------------------------------------------------------------

#[test]
fn test_gitvendors_list() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "foo"]
    url = https://example.com/foo.git
    branch = main

[vendor "bar"]
    url = https://example.com/bar.git
    base = deadbeef123
"#,
    );

    let mut vendors = super::vendors_from_config(&cfg).unwrap();
    vendors.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(vendors.len(), 2);

    assert_eq!(vendors[0].name, "bar");
    assert_eq!(vendors[0].url, "https://example.com/bar.git");
    assert_eq!(vendors[0].branch, None);
    assert_eq!(vendors[0].base.as_deref(), Some("deadbeef123"));

    assert_eq!(vendors[1].name, "foo");
    assert_eq!(vendors[1].url, "https://example.com/foo.git");
    assert_eq!(vendors[1].branch.as_deref(), Some("main"));
    assert_eq!(vendors[1].base, None);
}

#[test]
fn test_vendors_from_config_empty() {
    let (_tmp, cfg) = config_from_str("");
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert!(vendors.is_empty());
}

#[test]
fn test_vendors_from_config_single_entry() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "only"]
    url = https://example.com/only.git
"#,
    );
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert_eq!(vendors.len(), 1);
    assert_eq!(vendors[0].name, "only");
    assert_eq!(vendors[0].url, "https://example.com/only.git");
}

#[test]
fn test_vendors_from_config_ignores_non_vendor_sections() {
    let (_tmp, cfg) = config_from_str(
        r#"
[core]
    repositoryformatversion = 0

[vendor "legit"]
    url = https://example.com/legit.git
"#,
    );
    let vendors = super::vendors_from_config(&cfg).unwrap();
    assert_eq!(vendors.len(), 1);
    assert_eq!(vendors[0].name, "legit");
}

// ---------------------------------------------------------------------------
// bail_if_bare
// ---------------------------------------------------------------------------

#[test]
fn test_bail_if_bare_accepts_non_bare_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    assert!(super::bail_if_bare(&repo).is_ok());
}

#[test]
fn test_bail_if_bare_rejects_bare_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init_bare(tmp.path()).unwrap();
    let err = super::bail_if_bare(&repo).unwrap_err();
    assert!(
        err.message()
            .contains("bare repositories are not supported"),
        "unexpected error message: {}",
        err.message()
    );
}
