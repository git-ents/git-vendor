//! Property-based tests for `VendorConfig` and `PatternMapping`.

use git_vendor::{PatternMapping, VendorConfig, VendorEntry, VendorMode, VendorName};
use gix::bstr::BStr;
use proptest::prelude::*;

// ── Strategies ────────────────────────────────────────────────────────────────

fn name_strategy() -> impl Strategy<Value = String> {
    // git config subsection names can be arbitrary, but let's keep to
    // alphanumeric + hyphen to stay in simple territory.
    "[a-z][a-z0-9-]{0,15}".prop_map(|s| s)
}

fn url_strategy() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| format!("https://example.com/{s}.git"))
}

fn glob_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("src/**".to_owned()),
        Just("*.rs".to_owned()),
        Just("include/**".to_owned()),
        "[a-z]{2,6}/\\*\\*".prop_map(|s| s),
    ]
}

fn dest_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), "[a-z]{2,6}/".prop_map(Some),]
}

fn pattern_strategy() -> impl Strategy<Value = PatternMapping> {
    (glob_strategy(), dest_strategy())
        .prop_map(|(glob, destination)| PatternMapping { glob, destination })
}

fn mode_strategy() -> impl Strategy<Value = VendorMode> {
    prop_oneof![Just(VendorMode::Merge), Just(VendorMode::Squash)]
}

fn entry_strategy() -> impl Strategy<Value = VendorEntry> {
    (
        name_strategy(),
        url_strategy(),
        prop::option::of("[a-z]{2,8}".prop_map(|s| s)),
        prop::collection::vec(pattern_strategy(), 0..=3),
        mode_strategy(),
    )
        .prop_map(|(name, url, ref_name, patterns, mode)| VendorEntry {
            name: VendorName::new(name).expect("strategy yields valid names"),
            url,
            ref_name,
            base: None,
            patterns,
            mode,
        })
}

// ── PatternMapping invariants ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// `parse(to_raw(m))` is identical to `m`.
    #[test]
    fn pattern_round_trip(m in pattern_strategy()) {
        let raw = m.to_raw();
        let m2 = PatternMapping::parse(&raw);
        prop_assert_eq!(m2.glob, m.glob);
        prop_assert_eq!(m2.destination, m.destination);
    }

    /// Patterns without `:` always have no destination.
    #[test]
    fn no_colon_means_no_destination(glob in glob_strategy()) {
        prop_assume!(!glob.contains(':'));
        let m = PatternMapping::parse(&glob);
        prop_assert!(m.destination.is_none());
        prop_assert_eq!(m.glob, glob);
    }

    /// `local_path` always returns `Some` for paths that start with `literal_prefix`.
    #[test]
    fn local_path_some_when_prefix_matches(m in pattern_strategy(), suffix in "[a-z/]{0,8}") {
        let path = format!("{}{suffix}", m.literal_prefix());
        prop_assert!(m.local_path(BStr::new(&path)).is_some());
    }

    /// `local_path` starts with `destination` when it is set.
    #[test]
    fn local_path_has_destination_prefix(m in pattern_strategy(), suffix in "[a-z/]{0,8}") {
        let path = format!("{}{suffix}", m.literal_prefix());
        if let Some(dest) = &m.destination
            && let Some(local) = m.local_path(BStr::new(&path))
        {
            prop_assert!(
                local.starts_with(dest.as_bytes()),
                "local={local:?} dest={dest:?}"
            );
        }
    }

    /// `local_path` returns `None` when the upstream path doesn't start with `literal_prefix`.
    #[test]
    fn local_path_none_without_prefix(m in pattern_strategy()) {
        let prefix = m.literal_prefix();
        if !prefix.is_empty() {
            // A path that definitely doesn't start with the prefix.
            let bad = format!("ZZZZZ/{}", prefix);
            prop_assert!(m.local_path(BStr::new(&bad)).is_none());
        }
    }
}

// ── VendorConfig invariants ───────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    /// `insert` then `get` returns an entry with matching name and url.
    #[test]
    fn insert_then_get(entry in entry_strategy()) {
        let mut cfg = VendorConfig::parse("").expect("parse");
        cfg.insert(&entry).expect("insert");
        let got = cfg.get(&entry.name).expect("get").expect("present");
        prop_assert_eq!(got.name, entry.name);
        prop_assert_eq!(got.url, entry.url);
        prop_assert_eq!(got.ref_name, entry.ref_name);
        prop_assert_eq!(got.patterns.len(), entry.patterns.len());
        prop_assert_eq!(got.mode, entry.mode);
    }

    /// `insert` then `remove` leaves the config empty (for a single entry).
    #[test]
    fn insert_remove_leaves_empty(entry in entry_strategy()) {
        let mut cfg = VendorConfig::parse("").expect("parse");
        cfg.insert(&entry).expect("insert");
        let removed = cfg.remove(&entry.name).expect("remove");
        prop_assert!(removed);
        prop_assert!(cfg.get(&entry.name).expect("get").is_none());
        prop_assert_eq!(cfg.entries().expect("entries").len(), 0);
    }

    /// `insert` is idempotent: re-inserting the same entry keeps exactly one
    /// section and preserves the values.
    #[test]
    fn insert_is_idempotent(entry in entry_strategy()) {
        let mut cfg = VendorConfig::parse("").expect("parse");
        cfg.insert(&entry).expect("first insert");
        cfg.insert(&entry).expect("second insert");
        let entries = cfg.entries().expect("entries");
        prop_assert_eq!(entries.len(), 1);
        prop_assert_eq!(&entries[0].url, &entry.url);
    }

    /// Display/parse round-trip preserves all inserted entries.
    #[test]
    fn display_parse_round_trip(
        entries in prop::collection::vec(entry_strategy(), 1..=4)
            .prop_filter("unique names", |v| {
                let names: std::collections::HashSet<_> = v.iter().map(|e| &e.name).collect();
                names.len() == v.len()
            })
    ) {
        let mut cfg = VendorConfig::parse("").expect("parse");
        for e in &entries {
            cfg.insert(e).expect("insert");
        }
        let serialized = cfg.to_string();
        let cfg2 = VendorConfig::parse(&serialized).expect("re-parse");
        let got = cfg2.entries().expect("entries");
        prop_assert_eq!(got.len(), entries.len());
        for orig in &entries {
            let found = got.iter().find(|e| e.name == orig.name);
            prop_assert!(found.is_some(), "entry '{}' missing after round-trip", orig.name);
            let found = found.unwrap();
            prop_assert_eq!(&found.url, &orig.url);
            prop_assert_eq!(found.mode, orig.mode);
        }
    }
}
