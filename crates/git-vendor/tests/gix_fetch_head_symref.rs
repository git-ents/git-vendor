//! Pure-`gix` reproducer for the local-transport HEAD-symref resolution bug.
//!
//! `git fetch <url> +HEAD:<dst>` over the `file://` transport resolves the
//! source `HEAD` against the *calling* repository when the caller is non-bare
//! and shares the upstream's default branch name, instead of against the
//! remote. The destination ref ends up at the local tip; upstream objects are
//! never transferred.
//!
//! Real `git fetch` against the same two repositories resolves `HEAD` against
//! the remote, so this is a `gix` deviation, not a protocol ambiguity.
//!
//! File upstream at <https://github.com/GitoxideLabs/gitoxide/issues> if no
//! issue is open yet.
//!
//! `gix_fetch_head_resolves_against_local_repo_bug` is `#[should_panic]` so
//! CI stays green while the upstream bug stands; drop the attribute when
//! `gix` is fixed.

use std::path::Path;

fn git(args: &[&str], dir: &Path) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(std::process::Stdio::null())
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed in {dir:?}:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
}

fn rev_parse(dir: &Path, rev: &str) -> gix::ObjectId {
    let out = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("rev-parse");
    assert!(out.status.success(), "rev-parse {rev} failed in {dir:?}");
    let hex = std::str::from_utf8(&out.stdout).unwrap().trim();
    gix::ObjectId::from_hex(hex.as_bytes()).expect("valid hex")
}

/// Init a non-bare repo on `main` with one commit touching `marker`. Returns HEAD.
fn init_repo(dir: &Path, marker: &str) -> gix::ObjectId {
    git(&["init", "-b", "main"], dir);
    git(&["config", "user.email", "test@example.com"], dir);
    git(&["config", "user.name", "Test"], dir);
    std::fs::write(dir.join(marker), marker.as_bytes()).unwrap();
    git(&["add", "-A"], dir);
    git(&["commit", "-m", marker], dir);
    rev_parse(dir, "HEAD")
}

/// Fetch `+HEAD:refs/test/upstream` from `url` into `repo` using only `gix`,
/// return the OID the destination ref ends up at.
fn fetch_head(repo: &gix::Repository, url: &str) -> gix::ObjectId {
    repo.remote_at(url)
        .expect("remote_at")
        .with_refspecs(["+HEAD:refs/test/upstream"], gix::remote::Direction::Fetch)
        .expect("with_refspecs")
        .with_fetch_tags(gix::remote::fetch::Tags::None)
        .connect(gix::remote::Direction::Fetch)
        .expect("connect")
        .prepare_fetch(gix::progress::Discard, Default::default())
        .expect("prepare_fetch")
        .receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .expect("receive");
    repo.find_reference("refs/test/upstream")
        .expect("find ref")
        .peel_to_id()
        .expect("peel")
        .detach()
}

/// Oracle: real `git fetch` resolves `HEAD` against the remote. If this ever
/// fails, the fixture is broken — fix it before trusting the bug demo.
#[test]
fn git_cli_fetch_head_returns_upstream_tip() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let upstream_head = init_repo(upstream.path(), "upstream");
    init_repo(local.path(), "local");

    git(
        &[
            "fetch",
            upstream.path().to_str().unwrap(),
            "+HEAD:refs/test/upstream",
        ],
        local.path(),
    );

    assert_eq!(
        rev_parse(local.path(), "refs/test/upstream"),
        upstream_head,
        "git CLI must resolve HEAD against the remote",
    );
}

/// The bug. Same two repositories, same refspec — `gix` returns the *local*
/// HEAD instead of the upstream HEAD over its local file transport.
#[test]
#[should_panic(expected = "local-file transport resolved source `HEAD` against caller")]
fn gix_fetch_head_resolves_against_local_repo_bug() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let upstream_head = init_repo(upstream.path(), "upstream");
    let local_head = init_repo(local.path(), "local");
    assert_ne!(local_head, upstream_head, "fixture: heads must differ");

    let repo = gix::open(local.path()).expect("gix open");
    let got = fetch_head(&repo, upstream.path().to_str().unwrap());

    if got != upstream_head {
        panic!(
            "local-file transport resolved source `HEAD` against caller: \
             want upstream {upstream_head}, got {got} (local HEAD is {local_head})",
        );
    }
}

/// Control: against a *bare* local repo, the same call resolves correctly.
/// Isolates the bug to the non-bare-caller case and pins the working path.
#[test]
fn gix_fetch_head_into_bare_local_is_correct() {
    let upstream = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let upstream_head = init_repo(upstream.path(), "upstream");

    git(&["init", "--bare", "-b", "main"], local.path());
    let repo = gix::open(local.path()).expect("gix open");

    assert_eq!(
        fetch_head(&repo, upstream.path().to_str().unwrap()),
        upstream_head,
    );
}
