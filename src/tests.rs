//! Unit tests for `git-vendor`.

use git2::Config;
use std::io::Write;
use tempfile::NamedTempFile;

fn config_from_str(s: &str) -> (NamedTempFile, Config) {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(s.as_bytes()).unwrap();
    let cfg = Config::open(tmp.path()).unwrap();
    (tmp, cfg)
}

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
