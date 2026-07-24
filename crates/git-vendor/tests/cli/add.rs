//! End-to-end tests for `git-vendor add`.

use crate::support::{git, git_capture, init, vendor, write};

/// `add` must validate that every upstream path can be written as an
/// unquoted `.gitattributes` pattern *before* checking out any files, so an
/// unquotable path (e.g. containing a space) aborts cleanly instead of
/// leaving a half-applied checkout: vendored files present on disk and
/// staged in the index, but no `.gitvendors` entry and no `.gitattributes`
/// tracking. Regression: the check ran inside `track_vendor`, which is
/// called only *after* `checkout_vendor` has already mutated the working
/// tree and index.
#[test]
fn add_rejects_unquotable_path_without_partial_mutation() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    init(upstream.path());
    write(upstream.path(), "read me.txt", b"hello\n");
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "c"], upstream.path());

    init(local.path());
    write(local.path(), "README", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "init"], local.path());

    let url = upstream.path().to_str().unwrap();
    let out = vendor(&["add", url, "mylib"], local.path());
    assert!(
        !out.status.success(),
        "add must fail when an upstream path cannot be written as an unquoted \
         .gitattributes pattern",
    );

    assert!(
        !local.path().join("vendor").exists(),
        "no partial checkout should be left behind on disk",
    );

    let status = String::from_utf8(git_capture(&["status", "--porcelain"], local.path())).unwrap();
    assert!(
        status.is_empty(),
        "working tree/index must be untouched after the rejected add, but status was:\n{status}",
    );

    assert!(
        !local.path().join(".gitvendors").exists(),
        ".gitvendors must not be created by a rejected add",
    );
}
