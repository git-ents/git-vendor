//! End-to-end tests for `git-vendor update`.

use crate::support::{git, git_capture, init, vendor, write};

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

/// A conflicting `update` must still record `.gitattributes` tracking for the
/// vendor paths it introduces, so that once the user resolves the conflict and
/// commits, later `status`/`update`/`remove` can still find the vendor's files.
///
/// Regression: the conflict branch of `cmd_update` checked out the conflicted
/// tree and prepared the merge but skipped `reconcile_tracked_paths`, so a
/// newly vendored file landing alongside a conflict was never written to
/// `.gitattributes`.
#[test]
fn conflicting_update_tracks_new_paths() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    // Upstream v1: a single file that the local side will diverge from.
    init(upstream.path());
    write(upstream.path(), "hello.txt", b"v1\n");
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "v1"], upstream.path());

    // Local: vendor it (default prefix vendor/mylib/) and seal the add.
    init(local.path());
    write(local.path(), "README", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "init"], local.path());

    let url = upstream.path().to_str().unwrap();
    vendor_ok(&["add", url, "mylib"], local.path());
    git(&["commit", "-m", "vendor: add mylib"], local.path());

    // Local patch: diverge the vendored file so the next update conflicts.
    write(local.path(), "vendor/mylib/hello.txt", b"local\n");
    git(&["add", "-A"], local.path());
    git(&["commit", "-m", "local patch"], local.path());

    // Upstream v2: change the same file (forcing a conflict) and add a brand
    // new file (the path that must still get tracked).
    write(upstream.path(), "hello.txt", b"v2\n");
    write(upstream.path(), "extra.txt", b"new\n");
    git(&["add", "-A"], upstream.path());
    git(&["commit", "-m", "v2"], upstream.path());

    // Update must conflict (exit non-zero) and leave the merge in progress.
    let out = vendor(&["update", "mylib"], local.path());
    assert!(
        !out.status.success(),
        "update should report the conflict via non-zero exit:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );

    // Resolve the conflict and complete the merge the user is told to make.
    write(local.path(), "vendor/mylib/hello.txt", b"resolved\n");
    git(&["add", "vendor/mylib/hello.txt"], local.path());
    git(&["commit", "--no-edit"], local.path());

    let attrs =
        String::from_utf8(git_capture(&["show", "HEAD:.gitattributes"], local.path())).unwrap();
    assert!(
        attrs.contains("vendor/mylib/extra.txt"),
        "the newly vendored path must be tracked in .gitattributes, but was:\n{attrs}",
    );
}
