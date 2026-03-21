//! Unit tests for `git-vendor` — private-access only.

use git2::Config;
use std::io::Write;
use tempfile::{NamedTempFile, TempPath};

use super::{History, PatternMapping, VendorSource, bail_if_bare, vendors_from_config};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn config_from_str(s: &str) -> (TempPath, Config) {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(s.as_bytes()).unwrap();
    let path = tmp.into_temp_path();
    let cfg = Config::open(&path).unwrap();
    (path, cfg)
}

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
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    assert_eq!(vs.head_ref(), "refs/vendor/foo/head");
}

#[test]
fn test_head_ref_with_hyphens_and_underscores() {
    let vs = VendorSource {
        name: "my-cool_lib".into(),
        url: "https://example.com/lib.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    assert_eq!(vs.head_ref(), "refs/vendor/my-cool_lib/head");
}

// ---------------------------------------------------------------------------
// VendorSource::tracking_branch
// ---------------------------------------------------------------------------

#[test]
fn test_tracking_branch_defaults_to_head() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    assert_eq!(vs.tracking_branch(), "HEAD");
}

#[test]
fn test_tracking_branch_uses_explicit_branch() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        ref_name: Some("main".into()),
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    assert_eq!(vs.tracking_branch(), "main");
}

#[test]
fn test_tracking_branch_arbitrary_name() {
    let vs = VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        ref_name: Some("release/v2".into()),
        base: None,
        history: Default::default(),
        patterns: vec![],
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
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.foo.url").unwrap(),
        "https://example.com/foo.git"
    );
    assert!(cfg.get_string("vendor.foo.ref").is_err());
    assert!(cfg.get_string("vendor.foo.base").is_err());
}

#[test]
fn test_to_config_with_branch() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "bar".into(),
        url: "https://example.com/bar.git".into(),
        ref_name: Some("develop".into()),
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.bar.url").unwrap(),
        "https://example.com/bar.git"
    );
    assert_eq!(cfg.get_string("vendor.bar.ref").unwrap(), "develop");
    assert!(cfg.get_string("vendor.bar.base").is_err());
}

#[test]
fn test_to_config_with_base() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "baz".into(),
        url: "https://example.com/baz.git".into(),
        ref_name: None,
        base: Some("cafebabe".into()),
        history: Default::default(),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.baz.url").unwrap(),
        "https://example.com/baz.git"
    );
    assert!(cfg.get_string("vendor.baz.ref").is_err());
    assert_eq!(cfg.get_string("vendor.baz.base").unwrap(), "cafebabe");
}

#[test]
fn test_to_config_all_fields() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "full".into(),
        url: "https://example.com/full.git".into(),
        ref_name: Some("stable".into()),
        base: Some("deadbeef".into()),
        history: Default::default(),
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();

    assert_eq!(
        cfg.get_string("vendor.full.url").unwrap(),
        "https://example.com/full.git"
    );
    assert_eq!(cfg.get_string("vendor.full.ref").unwrap(), "stable");
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
    assert!(vs.ref_name.is_none());
    assert!(vs.base.is_none());
}

#[test]
fn test_from_config_with_branch() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "alpha"]
    url = https://example.com/alpha.git
    ref = next
"#,
    );
    let vs = VendorSource::from_config(&cfg, "alpha").unwrap().unwrap();

    assert_eq!(vs.name, "alpha");
    assert_eq!(vs.url, "https://example.com/alpha.git");
    assert_eq!(vs.ref_name.as_deref(), Some("next"));
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
    assert!(vs.ref_name.is_none());
    assert_eq!(vs.base.as_deref(), Some("1a2b3c4d"));
}

#[test]
fn test_from_config_all_fields() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "gamma"]
    url = https://example.com/gamma.git
    ref = release
    base = 0000ffff
"#,
    );
    let vs = VendorSource::from_config(&cfg, "gamma").unwrap().unwrap();

    assert_eq!(vs.name, "gamma");
    assert_eq!(vs.url, "https://example.com/gamma.git");
    assert_eq!(vs.ref_name.as_deref(), Some("release"));
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
        ref_name: Some("main".into()),
        base: Some("abc123def456".into()),
        history: Default::default(),
        patterns: vec![".config/".into(), ".github/".into()],
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "roundtrip")
        .unwrap()
        .unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert_eq!(restored.ref_name, original.ref_name);
    assert_eq!(restored.base, original.base);
    assert_eq!(restored.patterns, original.patterns);
}

#[test]
fn test_config_roundtrip_optional_fields_absent() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "minimal".into(),
        url: "https://example.com/minimal.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![],
    };
    original.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "minimal").unwrap().unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.url, original.url);
    assert!(restored.ref_name.is_none());
    assert!(restored.base.is_none());
    assert!(restored.patterns.is_empty());
}

#[test]
fn test_config_roundtrip_patterns_update() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "pat".into(),
        url: "https://example.com/pat.git".into(),
        ref_name: None,
        base: None,
        history: Default::default(),
        patterns: vec![".config/".into(), ".github/".into()],
    };
    original.to_config(&mut cfg).unwrap();

    let updated = VendorSource {
        patterns: vec![".github/".into(), "src/".into()],
        ..original.clone()
    };
    updated.to_config(&mut cfg).unwrap();

    let restored = VendorSource::from_config(&cfg, "pat").unwrap().unwrap();
    assert_eq!(
        restored.patterns,
        vec![".github/".to_string(), "src/".to_string()]
    );
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
    ref = main

[vendor "bar"]
    url = https://example.com/bar.git
    base = deadbeef123
"#,
    );

    let mut vendors = vendors_from_config(&cfg).unwrap();
    vendors.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(vendors.len(), 2);

    assert_eq!(vendors[0].name, "bar");
    assert_eq!(vendors[0].url, "https://example.com/bar.git");
    assert_eq!(vendors[0].ref_name, None);
    assert_eq!(vendors[0].base.as_deref(), Some("deadbeef123"));

    assert_eq!(vendors[1].name, "foo");
    assert_eq!(vendors[1].url, "https://example.com/foo.git");
    assert_eq!(vendors[1].ref_name.as_deref(), Some("main"));
    assert_eq!(vendors[1].base, None);
}

#[test]
fn test_vendors_from_config_empty() {
    let (_tmp, cfg) = config_from_str("");
    let vendors = vendors_from_config(&cfg).unwrap();
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
    let vendors = vendors_from_config(&cfg).unwrap();
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
    let vendors = vendors_from_config(&cfg).unwrap();
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
    assert!(bail_if_bare(&repo).is_ok());
}

#[test]
fn test_bail_if_bare_rejects_bare_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init_bare(tmp.path()).unwrap();
    let err = bail_if_bare(&repo).unwrap_err();
    assert!(
        err.message()
            .contains("bare repositories are not supported"),
        "unexpected error message: {}",
        err.message()
    );
}

// ---------------------------------------------------------------------------
// PatternMapping unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_pattern_mapping_parse_no_colon() {
    let m = PatternMapping::parse("src/**");
    assert_eq!(m.glob, "src/**");
    assert_eq!(m.destination, None);
}

#[test]
fn test_pattern_mapping_parse_with_destination() {
    let m = PatternMapping::parse("src/**:ext/");
    assert_eq!(m.glob, "src/**");
    assert_eq!(m.destination.as_deref(), Some("ext/"));
}

#[test]
fn test_pattern_mapping_parse_directory_shorthand() {
    let m = PatternMapping::parse("src/:ext/");
    assert_eq!(m.glob, "src/");
    assert_eq!(m.destination.as_deref(), Some("ext/"));
}

#[test]
fn test_pattern_mapping_to_raw_roundtrip() {
    let cases = ["src/**", "src/**:ext/", "docs/**:docs/upstream/", "**"];
    for raw in &cases {
        let m = PatternMapping::parse(raw);
        assert_eq!(m.to_raw(), *raw, "round-trip failed for {raw}");
    }
}

#[test]
fn test_literal_prefix_with_glob_star() {
    assert_eq!(PatternMapping::parse("src/**").literal_prefix(), "src/");
    assert_eq!(
        PatternMapping::parse("src/**/*.rs").literal_prefix(),
        "src/"
    );
    assert_eq!(PatternMapping::parse("**").literal_prefix(), "");
    assert_eq!(PatternMapping::parse("**/*.rs").literal_prefix(), "");
    assert_eq!(PatternMapping::parse("*.rs").literal_prefix(), "");
}

#[test]
fn test_literal_prefix_directory_shorthand() {
    assert_eq!(PatternMapping::parse("src/").literal_prefix(), "src/");
    assert_eq!(PatternMapping::parse("docs/").literal_prefix(), "docs/");
}

#[test]
fn test_local_path_no_destination_preserves_upstream() {
    let m = PatternMapping::parse("src/**");
    assert_eq!(
        m.local_path("src/lib/foo.rs"),
        Some("src/lib/foo.rs".into())
    );
    assert_eq!(m.local_path("src/mod.rs"), Some("src/mod.rs".into()));
}

#[test]
fn test_local_path_with_destination_strips_prefix() {
    let m = PatternMapping::parse("src/**:ext/");
    assert_eq!(
        m.local_path("src/lib/foo.rs"),
        Some("ext/lib/foo.rs".into())
    );
    assert_eq!(m.local_path("src/mod.rs"), Some("ext/mod.rs".into()));
}

#[test]
fn test_local_path_directory_shorthand_with_destination() {
    let m = PatternMapping::parse("src/:ext/");
    assert_eq!(
        m.local_path("src/lib/foo.rs"),
        Some("ext/lib/foo.rs".into())
    );
}

#[test]
fn test_local_path_glob_star_with_destination() {
    let m = PatternMapping::parse("**/*.rs:ext/");
    assert_eq!(
        m.local_path("src/lib/foo.rs"),
        Some("ext/src/lib/foo.rs".into())
    );
    let m2 = PatternMapping::parse("src/**/*.rs:ext/");
    assert_eq!(
        m2.local_path("src/lib/foo.rs"),
        Some("ext/lib/foo.rs".into())
    );
}

#[test]
fn test_local_path_prefix_mismatch_returns_none() {
    let m = PatternMapping::parse("src/:ext/");
    assert_eq!(m.local_path("other/foo.rs"), None);
}

#[test]
fn test_local_path_docs_to_upstream() {
    let m = PatternMapping::parse("docs/**:docs/upstream/");
    assert_eq!(
        m.local_path("docs/guide.md"),
        Some("docs/upstream/guide.md".into())
    );
}

// ---------------------------------------------------------------------------
// CommitMode serialization — to_config / from_config for the `commit` field
// ---------------------------------------------------------------------------

#[test]
fn test_commit_mode_default_is_squash() {
    assert_eq!(
        History::default(),
        History::Squash,
        "default CommitMode must be Squash"
    );
}

#[test]
fn test_to_config_commit_squash_is_omitted() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "sq".into(),
        url: "https://example.com/sq.git".into(),
        ref_name: None,
        base: None,
        history: History::Squash,
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();
    assert!(
        cfg.get_string("vendor.sq.history").is_err(),
        "squash (default) must not be written to config"
    );
}

#[test]
fn test_to_config_commit_linear_is_written() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "lin".into(),
        url: "https://example.com/lin.git".into(),
        ref_name: None,
        base: None,
        history: History::Linear,
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();
    assert_eq!(cfg.get_string("vendor.lin.history").unwrap(), "linear");
}

#[test]
fn test_to_config_commit_replay_is_written() {
    let (_tmp, mut cfg) = empty_config();
    let vs = VendorSource {
        name: "rep".into(),
        url: "https://example.com/rep.git".into(),
        ref_name: None,
        base: None,
        history: History::Replay,
        patterns: vec![],
    };
    vs.to_config(&mut cfg).unwrap();
    assert_eq!(cfg.get_string("vendor.rep.history").unwrap(), "replay");
}

#[test]
fn test_from_config_commit_absent_defaults_to_squash() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "sq"]
    url = https://example.com/sq.git
"#,
    );
    let vs = VendorSource::from_config(&cfg, "sq").unwrap().unwrap();
    assert_eq!(vs.history, History::Squash);
}

#[test]
fn test_from_config_commit_linear() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "lin"]
    url = https://example.com/lin.git
    history = linear
"#,
    );
    let vs = VendorSource::from_config(&cfg, "lin").unwrap().unwrap();
    assert_eq!(vs.history, History::Linear);
}

#[test]
fn test_from_config_commit_replay() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "rep"]
    url = https://example.com/rep.git
    history = replay
"#,
    );
    let vs = VendorSource::from_config(&cfg, "rep").unwrap().unwrap();
    assert_eq!(vs.history, History::Replay);
}

#[test]
fn test_from_config_commit_unknown_falls_back_to_squash() {
    let (_tmp, cfg) = config_from_str(
        r#"
[vendor "unk"]
    url = https://example.com/unk.git
    history = bogus
"#,
    );
    let vs = VendorSource::from_config(&cfg, "unk").unwrap().unwrap();
    assert_eq!(
        vs.history,
        History::Squash,
        "unrecognized commit value must fall back to Squash"
    );
}

#[test]
fn test_commit_mode_roundtrip_linear() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "rt".into(),
        url: "https://example.com/rt.git".into(),
        ref_name: None,
        base: None,
        history: History::Linear,
        patterns: vec![],
    };
    original.to_config(&mut cfg).unwrap();
    let restored = VendorSource::from_config(&cfg, "rt").unwrap().unwrap();
    assert_eq!(restored.history, History::Linear);
}

#[test]
fn test_commit_mode_roundtrip_replay() {
    let (_tmp, mut cfg) = empty_config();
    let original = VendorSource {
        name: "rtr".into(),
        url: "https://example.com/rtr.git".into(),
        ref_name: None,
        base: None,
        history: History::Replay,
        patterns: vec![],
    };
    original.to_config(&mut cfg).unwrap();
    let restored = VendorSource::from_config(&cfg, "rtr").unwrap().unwrap();
    assert_eq!(restored.history, History::Replay);
}
