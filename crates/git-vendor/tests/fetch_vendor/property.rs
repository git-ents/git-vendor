//! Property-based tests for invariants relied upon by `fetch_vendor`.

use git_vendor::{VendorEntry, VendorName};
use proptest::prelude::*;

// ── Strategies ────────────────────────────────────────────────────────────────

fn name_strategy() -> impl Strategy<Value = VendorName> {
    "[a-z][a-z0-9-]{0,15}".prop_map(|s| VendorName::new(s).expect("strategy yields valid names"))
}

fn ref_name_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), "[a-z][a-z0-9-]{0,10}".prop_map(Some)]
}

fn entry_strategy() -> impl Strategy<Value = VendorEntry> {
    (name_strategy(), ref_name_strategy()).prop_map(|(name, ref_name)| VendorEntry {
        name,
        url: "https://example.com/repo.git".to_owned(),
        ref_name,
        base: None,
        patterns: vec![],
    })
}

// ── Invariants ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// For any valid vendor name, `vendor_ref()` stays within the
    /// `refs/vendor/` namespace and ends with the name. This is the invariant
    /// the `VendorName` newtype exists to guarantee: a name can never escape
    /// the namespace and collide with `refs/heads/` or `refs/tags/`.
    #[test]
    fn vendor_ref_is_under_refs_vendor(entry in entry_strategy()) {
        let r = entry.vendor_ref();
        prop_assert!(r.starts_with("refs/vendor/"), "got: {r:?}");
        prop_assert!(r.ends_with(entry.name.as_str()), "got: {r:?}");
        prop_assert!(
            !r["refs/vendor/".len()..].contains('/'),
            "name must be a single path component: {r:?}",
        );
    }
}
