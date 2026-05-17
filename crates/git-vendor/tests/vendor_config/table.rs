//! Table-driven tests for `VendorConfig` and `PatternMapping`.

use git_vendor::{PatternMapping, VendorConfig, VendorEntry};
use rstest::rstest;

// ── PatternMapping ────────────────────────────────────────────────────────────

#[rstest]
#[case("src/**", "src/**", None)]
#[case("src/**:third_party/", "src/**", Some("third_party/"))]
#[case("*.rs:vendor/", "*.rs", Some("vendor/"))]
#[case("a/b:c/d", "a/b", Some("c/d"))]
#[case("no-colon", "no-colon", None)]
fn pattern_parse(#[case] raw: &str, #[case] glob: &str, #[case] dest: Option<&str>) {
    let m = PatternMapping::parse(raw);
    assert_eq!(m.glob, glob);
    assert_eq!(m.destination.as_deref(), dest);
}

#[rstest]
#[case("src/**", "src/")]
#[case("*.rs", "")]
#[case("a/b/c", "a/b/c")]
#[case("a/b/c/**", "a/b/c/")]
#[case("?file", "")]
#[case("[abc]", "")]
#[case("{a,b}", "")]
fn literal_prefix(#[case] glob: &str, #[case] want: &str) {
    let m = PatternMapping {
        glob: glob.to_owned(),
        destination: None,
    };
    assert_eq!(m.literal_prefix(), want);
}

#[rstest]
#[case("src/**", None, "src/foo.rs", Some("foo.rs"))]
#[case(
    "src/**",
    Some("third_party/"),
    "src/foo.rs",
    Some("third_party/foo.rs")
)]
#[case("src/**", Some("tp/"), "other/foo.rs", None)]
#[case("*.rs", None, "foo.rs", Some("foo.rs"))]
#[case("*.rs", Some("vendor/"), "foo.rs", Some("vendor/foo.rs"))]
#[case("a/b/c", None, "a/b/c", Some(""))]
fn local_path(
    #[case] glob: &str,
    #[case] dest: Option<&str>,
    #[case] upstream: &str,
    #[case] want: Option<&str>,
) {
    let m = PatternMapping {
        glob: glob.to_owned(),
        destination: dest.map(str::to_owned),
    };
    assert_eq!(m.local_path(upstream).as_deref(), want);
}

// ── VendorConfig parse / entries ─────────────────────────────────────────────

fn simple_config() -> &'static str {
    r#"[vendor "lib-a"]
	url = https://example.com/lib-a.git
	ref = main
	pattern = src/**:third_party/lib-a/
"#
}

#[test]
fn parse_returns_one_entry() {
    let cfg = VendorConfig::parse(simple_config()).expect("parse");
    let entries = cfg.entries().expect("entries");
    assert_eq!(entries.len(), 1);

    let e = &entries[0];
    assert_eq!(e.name, "lib-a");
    assert_eq!(e.url, "https://example.com/lib-a.git");
    assert_eq!(e.ref_name.as_deref(), Some("main"));
    assert!(e.base.is_none());
    assert_eq!(entries[0].patterns.len(), 1);
    assert_eq!(entries[0].patterns[0].glob, "src/**");
    assert_eq!(
        entries[0].patterns[0].destination.as_deref(),
        Some("third_party/lib-a/")
    );
}

#[test]
fn parse_multiple_patterns() {
    let cfg = VendorConfig::parse(
        r#"[vendor "foo"]
	url = https://example.com/foo.git
	pattern = include/**
	pattern = README.md:docs/foo-README.md
"#,
    )
    .expect("parse");
    let entries = cfg.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.patterns.len(), 2);
    assert_eq!(e.patterns[0].glob, "include/**");
    assert!(e.patterns[0].destination.is_none());
    assert_eq!(e.patterns[1].glob, "README.md");
    assert_eq!(
        e.patterns[1].destination.as_deref(),
        Some("docs/foo-README.md")
    );
}

#[test]
fn parse_multiple_vendors() {
    let cfg = VendorConfig::parse(
        r#"[vendor "alpha"]
	url = https://example.com/alpha.git
[vendor "beta"]
	url = https://example.com/beta.git
"#,
    )
    .expect("parse");
    let entries = cfg.entries().expect("entries");
    assert_eq!(entries.len(), 2);
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn empty_config_yields_no_entries() {
    let cfg = VendorConfig::parse("").expect("parse");
    assert_eq!(cfg.entries().expect("entries").len(), 0);
}

// ── get ───────────────────────────────────────────────────────────────────────

#[test]
fn get_returns_entry_by_name() {
    let cfg = VendorConfig::parse(simple_config()).expect("parse");
    let e = cfg.get("lib-a").expect("get").expect("present");
    assert_eq!(e.name, "lib-a");
}

#[test]
fn get_missing_returns_none() {
    let cfg = VendorConfig::parse(simple_config()).expect("parse");
    assert!(cfg.get("nonexistent").expect("get").is_none());
}

// ── insert ────────────────────────────────────────────────────────────────────

fn make_entry(name: &str) -> VendorEntry {
    VendorEntry {
        name: name.to_owned(),
        url: format!("https://example.com/{name}.git"),
        ref_name: Some("main".to_owned()),
        base: None,
        patterns: vec![PatternMapping {
            glob: "src/**".to_owned(),
            destination: Some("third_party/".to_owned()),
        }],
    }
}

#[test]
fn insert_then_get_round_trips() {
    let mut cfg = VendorConfig::parse("").expect("parse");
    let entry = make_entry("mylib");
    cfg.insert(&entry).expect("insert");

    let got = cfg.get("mylib").expect("get").expect("present");
    assert_eq!(got.name, entry.name);
    assert_eq!(got.url, entry.url);
    assert_eq!(got.ref_name, entry.ref_name);
    assert_eq!(got.patterns.len(), 1);
    assert_eq!(got.patterns[0].to_raw(), "src/**:third_party/");
}

#[test]
fn insert_replaces_existing_entry() {
    let mut cfg = VendorConfig::parse(simple_config()).expect("parse");
    let mut entry = make_entry("lib-a");
    entry.url = "https://new.example.com/lib-a.git".to_owned();
    cfg.insert(&entry).expect("insert");

    let entries = cfg.entries().expect("entries");
    assert_eq!(entries.len(), 1, "no duplicate entries");
    assert_eq!(entries[0].url, "https://new.example.com/lib-a.git");
}

// ── remove ────────────────────────────────────────────────────────────────────

#[test]
fn remove_existing_returns_true() {
    let mut cfg = VendorConfig::parse(simple_config()).expect("parse");
    assert!(cfg.remove("lib-a").expect("remove"));
    assert_eq!(cfg.entries().expect("entries").len(), 0);
}

#[test]
fn remove_missing_returns_false() {
    let mut cfg = VendorConfig::parse(simple_config()).expect("parse");
    assert!(!cfg.remove("nonexistent").expect("remove"));
}

// ── Display / round-trip ──────────────────────────────────────────────────────

#[test]
fn display_round_trips_entries() {
    let mut cfg = VendorConfig::parse("").expect("parse");
    cfg.insert(&make_entry("lib-x")).expect("insert");
    cfg.insert(&make_entry("lib-y")).expect("insert");

    let serialized = cfg.to_string();
    let cfg2 = VendorConfig::parse(&serialized).expect("re-parse");
    let entries = cfg2.entries().expect("entries");
    assert_eq!(entries.len(), 2);
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"lib-x"));
    assert!(names.contains(&"lib-y"));
}

// ── open ──────────────────────────────────────────────────────────────────────

#[test]
fn open_reads_file_from_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".gitvendors");
    std::fs::write(&path, simple_config()).expect("write");

    let cfg = VendorConfig::open(&path).expect("open");
    let entries = cfg.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "lib-a");
}
