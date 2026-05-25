//! Property-based tests for invariants of `base_tree`.
//!
//! Whenever `entry.base` is `None`, `base_tree` returns `None` before it ever
//! touches the repository; the `Some(base)` path delegates to `upstream_tree`,
//! which is not yet implemented. The only property that is total and free of
//! side effects is therefore that a `None` base always yields `None`. The
//! `ref_name` and `patterns` fields never reach a code path here, so only the
//! name is fuzzed, matching the `vendor_tip` properties. The recorded-base
//! path will get its own property once `upstream_tree` lands.

use git_vendor::{VendorEntry, VendorMode, VendorName, VendorRepository as _};
use proptest::prelude::*;

fn name_strategy() -> impl Strategy<Value = VendorName> {
    "[a-z][a-z0-9-]{0,15}".prop_map(|s| VendorName::new(s).expect("strategy yields valid names"))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// For any entry without a recorded base, `base_tree` is `Ok(None)`,
    /// independent of name and repository state.
    #[test]
    fn baseless_entry_has_no_base_tree(name in name_strategy()) {
        let entry = VendorEntry {
            name,
            url: "https://example.com/repo.git".to_owned(),
            ref_name: None,
            base: None,
            patterns: vec![],
            mode: VendorMode::Merge,
        };
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).expect("init bare");
        prop_assert_eq!(repo.base_tree(&entry).expect("base_tree"), None);
    }
}
