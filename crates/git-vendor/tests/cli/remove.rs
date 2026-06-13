//! End-to-end tests for `git-vendor remove`.

use crate::support::{git, git_capture, init, make_upstream, vendor, write};

/// Run a vendor subcommand and assert it exited zero, surfacing stderr on
/// failure.
fn vendor_ok(args: &[&str], dir: &std::path::Path) {
    let out = vendor(args, dir);
    assert!(
        out.status.success(),
        "git-vendor {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// `remove` must stage the rewritten `.gitvendors` so the user's next commit
/// records the deletion. Regression: `cmd_remove` wrote `.gitvendors` to the
/// working tree but never staged it, leaving the index pointing at the old
/// blob that still listed the removed vendor.
#[test]
fn remove_stages_updated_gitvendors() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());

    init(local.path());
    write(local.path(), "README", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "init"], local.path());

    let url = upstream.path().to_str().unwrap();
    vendor_ok(&["add", url, "mylib"], local.path());
    // `add` leaves the merge staged with MERGE_HEAD set; seal it.
    git(&["commit", "-m", "vendor: add mylib"], local.path());

    let committed =
        String::from_utf8(git_capture(&["show", "HEAD:.gitvendors"], local.path())).unwrap();
    assert!(
        committed.contains("mylib"),
        "precondition: committed .gitvendors must list the vendor:\n{committed}",
    );

    vendor_ok(&["remove", "mylib"], local.path());

    let staged = String::from_utf8(git_capture(&["show", ":.gitvendors"], local.path())).unwrap();
    assert!(
        !staged.contains("mylib"),
        "staged .gitvendors must not reference the removed vendor, but was:\n{staged}",
    );
}
