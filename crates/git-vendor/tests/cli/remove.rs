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

/// `remove --keep-files` must still untrack the vendor's `.gitattributes`
/// entries, even though it leaves the files themselves on disk. Regression:
/// `untrack_vendor` was called only in the `!keep_files` branch, so
/// `--keep-files` left stale `vendor=<name>` lines behind.
#[test]
fn remove_keep_files_still_untracks_gitattributes() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());

    init(local.path());
    write(local.path(), "README", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "init"], local.path());

    let url = upstream.path().to_str().unwrap();
    vendor_ok(&["add", url, "mylib"], local.path());
    git(&["commit", "-m", "vendor: add mylib"], local.path());

    vendor_ok(&["remove", "--keep-files", "mylib"], local.path());

    assert!(
        local.path().join("vendor/mylib/hello.txt").exists(),
        "--keep-files must leave the vendored files on disk",
    );

    let staged =
        String::from_utf8(git_capture(&["show", ":.gitattributes"], local.path())).unwrap();
    assert!(
        !staged.contains("vendor=mylib"),
        "staged .gitattributes must not reference the removed vendor, but was:\n{staged}",
    );
}

/// `remove` on a born HEAD but *before committing the add* must still remove
/// the vendor's files. Regression: `remove` resolved paths only from HEAD's
/// tree, which — for a vendor added but not yet committed — carries no
/// `vendor=<name>` tracking, so it reported success while leaving the files on
/// disk, in the index, and in `.gitattributes`.
#[test]
fn remove_after_uncommitted_add_removes_vendor() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());

    init(local.path());
    write(local.path(), "README", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "init"], local.path());

    let url = upstream.path().to_str().unwrap();
    vendor_ok(&["add", url, "mylib"], local.path());
    // Deliberately do NOT commit the add — the vendor lives only in the index.
    vendor_ok(&["remove", "mylib"], local.path());

    assert!(
        !local.path().join("vendor/mylib/hello.txt").exists(),
        "remove must delete files from an uncommitted add",
    );
    let tracked = String::from_utf8(git_capture(&["ls-files"], local.path())).unwrap();
    assert!(
        !tracked.lines().any(|l| l.starts_with("vendor/mylib/")),
        "no vendor/mylib/* path should remain in the index:\n{tracked}",
    );
    let staged =
        String::from_utf8(git_capture(&["show", ":.gitattributes"], local.path())).unwrap();
    assert!(
        !staged.contains("vendor=mylib"),
        "staged .gitattributes must not reference the removed vendor:\n{staged}",
    );
}

/// `remove` on an unborn HEAD (before the first commit) must actually remove
/// the vendor, not just its `.gitvendors` entry. Regression: file deletion,
/// `untrack_vendor`, and index cleanup were all gated on `repo.head_commit()`
/// succeeding, so on an unborn HEAD `remove` reported success while leaving
/// vendored files on disk, in the index, and in `.gitattributes`.
#[test]
fn remove_before_first_commit_removes_vendor() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    make_upstream(upstream.path());
    init(local.path());

    let url = upstream.path().to_str().unwrap();
    vendor_ok(&["add", url, "mylib"], local.path());

    vendor_ok(&["remove", "mylib"], local.path());

    assert!(
        !local.path().join("vendor/mylib/hello.txt").exists(),
        "remove must delete vendored files even on an unborn HEAD",
    );

    let tracked = String::from_utf8(git_capture(&["ls-files"], local.path())).unwrap();
    assert!(
        !tracked.lines().any(|l| l.starts_with("vendor/mylib/")),
        "no vendor/mylib/* path should remain in the index, but ls-files was:\n{tracked}",
    );

    if local.path().join(".gitattributes").exists() {
        let contents = std::fs::read_to_string(local.path().join(".gitattributes")).unwrap();
        assert!(
            !contents.contains("vendor=mylib"),
            ".gitattributes must not reference the removed vendor, but was:\n{contents}",
        );
    }
}
