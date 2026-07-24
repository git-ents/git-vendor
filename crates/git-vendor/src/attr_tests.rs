use rstest::rstest;

use super::{check_attr_pattern, split_attr_line};

#[rstest]
#[case(b"vendor/a.txt")]
#[case(b"src/lib.rs")]
#[case(b"deep/nested/path.txt")]
// `#`, `!`, and glob metacharacters are escaped on write, not rejected.
#[case(b"#readme")]
#[case(b"!bang")]
#[case(b"a*.txt")]
fn check_plain_path_ok(#[case] path: &[u8]) {
    assert!(check_attr_pattern(path).is_ok());
}

#[rstest]
#[case(b"vendor/a b.txt")]
#[case(b"say \"hi\"")]
#[case(b"a\tb")]
#[case(b"a\x01b")]
#[case(b"a\x7fb")]
fn check_path_with_special_chars_errors(#[case] path: &[u8]) {
    assert!(check_attr_pattern(path).is_err());
}

#[rstest]
#[case(b"vendor/a.txt vendor=mylib", b"vendor/a.txt", b"vendor=mylib")]
#[case(b"vendor/a.txt\tvendor=mylib", b"vendor/a.txt", b"vendor=mylib")]
#[case(b"\"vendor/a b.txt\" vendor=mylib", b"vendor/a b.txt", b"vendor=mylib")]
#[case(b"\"say \\\"hi\\\"\" vendor=mylib", b"say \"hi\"", b"vendor=mylib")]
fn split_line(#[case] line: &[u8], #[case] pattern: &[u8], #[case] attr: &[u8]) {
    let (p, a) = split_attr_line(line).unwrap();
    assert_eq!(p.as_ref(), pattern);
    assert_eq!(a, attr);
}

#[rstest]
#[case(b"vendor/a.txt")]
#[case(b"")]
#[case(b"# comment line")]
#[case(b"# vendor=mylib")]
#[case(b"  ")]
#[case(b" vendor/a.txt attr")]
fn split_no_attr_returns_none(#[case] line: &[u8]) {
    assert!(split_attr_line(line).is_none());
}
