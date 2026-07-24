use super::*;
use std::path::Path;

fn git(args: &[&str], dir: &Path) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn test_entry() -> VendorEntry {
    VendorEntry {
        name: VendorName::new("mylib").unwrap(),
        url: "unused".to_owned(),
        ref_name: None,
        base: None,
        patterns: Vec::new(),
        mode: VendorMode::Merge,
    }
}

/// `reconcile_tracked_paths` must not collapse a genuine unresolved merge
/// conflict on `.gitattributes` itself into a single resolved stage-0
/// entry: doing so would let `git commit` silently record the
/// conflict-marker text as if it were normal content. Regression: the
/// function always called `track_vendor`, which reads and rewrites
/// `.gitattributes`, then `stage_gitattributes` unconditionally replaced
/// whatever stages were there with one stage-0 entry.
#[test]
fn skips_write_when_gitattributes_itself_is_conflicted() {
    use git_vendor::{ConflictStages, VendorMerge};

    let dir = tempfile::tempdir().unwrap();
    git(&["init", "-q", "-b", "main"], dir.path());
    git(&["config", "user.email", "t@example.com"], dir.path());
    git(&["config", "user.name", "T"], dir.path());
    std::fs::write(dir.path().join(".gitattributes"), "* text=auto\n").unwrap();
    git(&["add", "."], dir.path());
    git(&["commit", "-q", "-m", "init"], dir.path());

    let repo = gix::open(dir.path()).expect("gix open");
    let entry = test_entry();

    let base_blob = repo
        .write_object(gix::objs::BlobRef {
            data: b"* text=auto\n",
        })
        .expect("write base blob")
        .detach();
    let ours_blob = repo
        .write_object(gix::objs::BlobRef {
            data: b"* text=auto\nours=1\n",
        })
        .expect("write ours blob")
        .detach();
    let theirs_blob = repo
        .write_object(gix::objs::BlobRef {
            data: b"* text=auto\ntheirs=1\n",
        })
        .expect("write theirs blob")
        .detach();
    let conflict_marker_blob = repo
        .write_object(gix::objs::BlobRef {
            data: b"<<<<<<< ours\nours=1\n=======\ntheirs=1\n>>>>>>> theirs\n",
        })
        .expect("write conflict-marker blob")
        .detach();

    let blob_mode = gix::objs::tree::EntryMode::from(gix::objs::tree::EntryKind::Blob);
    let head_tree = repo
        .head_commit()
        .expect("head")
        .tree_id()
        .expect("tree")
        .detach();
    let mut editor = repo
        .find_tree(head_tree)
        .expect("find tree")
        .edit()
        .expect("edit");
    editor
        .upsert(
            ".gitattributes",
            gix::objs::tree::EntryKind::Blob,
            conflict_marker_blob,
        )
        .expect("upsert conflicted .gitattributes");
    let result_tree = editor.write().expect("write tree").detach();

    let merge = VendorMerge {
        upstream_commit: repo.head_commit().expect("head").id().detach(),
        ancestor_tree: None,
        result_tree,
        conflicts: vec![ConflictStages {
            path: ".gitattributes".to_owned(),
            stages: [
                Some((blob_mode, base_blob)),
                Some((blob_mode, ours_blob)),
                Some((blob_mode, theirs_blob)),
            ],
        }],
    };

    repo.checkout_vendor_conflicted(&entry, &merge)
        .expect("checkout_vendor_conflicted");

    let new_paths = tree_paths(&repo, merge.result_tree).expect("tree_paths");
    let mut io = Io {
        out: Box::new(Vec::new()),
        err: Box::new(Vec::new()),
    };
    let result = reconcile_tracked_paths(&repo, &entry, &[], &new_paths, &mut io)
        .expect("reconcile_tracked_paths");
    assert!(
        result.is_none(),
        "must skip and return None when .gitattributes itself is conflicted",
    );

    let index = repo.open_index().expect("open_index");
    use gix::bstr::ByteSlice as _;
    let stages: Vec<_> = index
        .entries()
        .iter()
        .filter(|e| e.path(&index) == b".gitattributes".as_bstr())
        .map(|e| e.flags.stage())
        .collect();
    assert_eq!(
        stages.len(),
        3,
        "all three unmerged stages must survive, got {stages:?}",
    );
    assert!(
        stages
            .iter()
            .all(|s| *s != gix::index::entry::Stage::Unconflicted),
        "no stage should have been collapsed to stage 0, got {stages:?}",
    );
}
