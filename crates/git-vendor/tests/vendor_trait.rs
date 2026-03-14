//! Integration tests for the `Vendor` trait implementation on `git2::Repository`.

use git_vendor::Vendor;
use git2::{Repository, Signature};
use std::fs;
use std::path::Path;

use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Remote repo fixture (acts as the upstream for fetch tests)
// ---------------------------------------------------------------------------

struct RemoteRepo {
    /// Kept alive so the temp directory is not deleted.
    _dir: TempDir,
    pub repo: Repository,
}

impl RemoteRepo {
    /// Initialize a bare repository that will act as the upstream remote.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init_bare(dir.path()).unwrap();
        Self { _dir: dir, repo }
    }

    /// `file://`-prefixed URL that libgit2 can fetch from without SSH/HTTPS.
    fn url(&self) -> String {
        // On Windows, Path::display() uses backslashes, which libgit2 then
        // rejects as "invalid escape" when reading the value back from git
        // config. Convert to forward slashes for a valid file:// URL.
        // Windows paths also start with a drive letter ("C:/...") rather than
        // '/', so an extra leading slash is needed to form a proper file URL.
        let path = self._dir.path().to_string_lossy().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{}", path)
        } else {
            format!("file:///{}", path)
        }
    }

    fn sig() -> Signature<'static> {
        Signature::now("Remote User", "remote@example.com").unwrap()
    }

    /// Write a commit directly into the bare repo, advancing `refs/heads/main`
    /// (and therefore `HEAD`).  Returns the new commit OID.
    fn make_commit(&self, files: &[(&str, &[u8])], message: &str) -> git2::Oid {
        let sig = Self::sig();
        let mut tb = self.repo.treebuilder(None).unwrap();
        for (name, content) in files {
            let blob = self.repo.blob(content).unwrap();
            tb.insert(name, blob, 0o100644).unwrap();
        }
        let tree_oid = tb.write().unwrap();
        let tree = self.repo.find_tree(tree_oid).unwrap();

        let head_commit = self
            .repo
            .find_reference("refs/heads/main")
            .ok()
            .map(|r| r.peel_to_commit().unwrap());
        let parents: Vec<&git2::Commit<'_>> = head_commit.iter().collect();

        let oid = self
            .repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                message,
                &tree,
                &parents,
            )
            .unwrap();

        // Make sure HEAD points at main so "HEAD" refspec resolves correctly.
        self.repo
            .reference_symbolic("HEAD", "refs/heads/main", true, "init")
            .ok();

        oid
    }

    /// Push a commit onto an explicit branch ref (e.g. `"refs/heads/stable"`).
    fn make_commit_on(
        &self,
        branch_ref: &str,
        files: &[(&str, &[u8])],
        message: &str,
    ) -> git2::Oid {
        let sig = Self::sig();
        let mut tb = self.repo.treebuilder(None).unwrap();
        for (name, content) in files {
            let blob = self.repo.blob(content).unwrap();
            tb.insert(name, blob, 0o100644).unwrap();
        }
        let tree_oid = tb.write().unwrap();
        let tree = self.repo.find_tree(tree_oid).unwrap();

        let head_commit = self
            .repo
            .find_reference(branch_ref)
            .ok()
            .map(|r| r.peel_to_commit().unwrap());
        let parents: Vec<&git2::Commit<'_>> = head_commit.iter().collect();

        self.repo
            .commit(Some(branch_ref), &sig, &sig, message, &tree, &parents)
            .unwrap()
    }
}

// ---------------------------------------------------------------------------
// Test fixture
// ---------------------------------------------------------------------------

struct TestRepo {
    /// Kept alive so the temp directory is not deleted while the repo is open.
    _dir: TempDir,
    pub repo: Repository,
}

impl TestRepo {
    /// Initialize a fresh non-bare repository with a pre-configured identity.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Test User").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
        }

        Self { _dir: dir, repo }
    }

    /// Write `content` into `.gitvendors`.
    fn write_gitvendors(&self, content: &str) {
        let path = self.repo.workdir().unwrap().join(".gitvendors");
        fs::write(path, content).unwrap();
    }

    /// Write `content` into `.gitattributes` in the working directory.
    fn write_gitattributes(&self, content: &str) {
        let workdir = self.repo.workdir().unwrap();
        let path = workdir.join(".gitattributes");
        fs::write(path, content).unwrap();
    }

    /// Write a file into the working directory.
    fn write_file(&self, name: &str, content: &[u8]) {
        let workdir = self.repo.workdir().unwrap();
        let path = workdir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn sig(&self) -> Signature<'static> {
        Signature::now("Test User", "test@example.com").unwrap()
    }

    /// Build a tree from `(path, content)` pairs and return its OID.
    fn make_tree(&self, files: &[(&str, &[u8])]) -> git2::Oid {
        let mut tb = self.repo.treebuilder(None).unwrap();
        for (name, content) in files {
            let blob = self.repo.blob(content).unwrap();
            tb.insert(name, blob, 0o100644).unwrap();
        }
        tb.write().unwrap()
    }

    /// Create a commit (not updating any ref) and return its OID.
    fn make_commit_oid(
        &self,
        tree_oid: git2::Oid,
        message: &str,
        parents: &[git2::Oid],
    ) -> git2::Oid {
        let sig = self.sig();
        let tree = self.repo.find_tree(tree_oid).unwrap();
        let parent_commits: Vec<git2::Commit<'_>> = parents
            .iter()
            .map(|&oid| self.repo.find_commit(oid).unwrap())
            .collect();
        let parent_refs: Vec<&git2::Commit<'_>> = parent_commits.iter().collect();
        self.repo
            .commit(None, &sig, &sig, message, &tree, &parent_refs)
            .unwrap()
    }

    /// Create a commit and point `HEAD` (via `refs/heads/main`) at it.
    fn make_head_commit(&self, files: &[(&str, &[u8])]) -> git2::Oid {
        // Stage files so the index is consistent with the working tree.
        for (name, content) in files {
            self.write_file(name, content);
        }

        let mut index = self.repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();

        let tree_oid = index.write_tree().unwrap();
        let sig = self.sig();
        let tree = self.repo.find_tree(tree_oid).unwrap();

        let head_oid = match self.repo.head() {
            Ok(head) => {
                let parent = head.peel_to_commit().unwrap();
                self.repo
                    .commit(Some("HEAD"), &sig, &sig, "test commit", &tree, &[&parent])
                    .unwrap()
            }
            Err(_) => {
                // First commit — no parent.
                self.repo
                    .commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                    .unwrap()
            }
        };

        head_oid
    }

    /// Point `refs/vendor/<name>/head` at `oid`.
    fn set_vendor_ref(&self, name: &str, oid: git2::Oid) {
        let refname = format!("refs/vendor/{}/head", name);
        self.repo.reference(&refname, oid, true, "test").unwrap();
    }
}

// ---------------------------------------------------------------------------
// vendor_config
// ---------------------------------------------------------------------------

#[test]
fn test_vendor_config_reads_gitvendors_file() {
    let tr = TestRepo::new();
    tr.write_gitvendors(
        r#"
[vendor "mylib"]
    url = https://example.com/mylib.git
"#,
    );

    let cfg = tr.repo.vendor_config().unwrap();
    assert_eq!(
        cfg.get_string("vendor.mylib.url").unwrap(),
        "https://example.com/mylib.git"
    );
}

#[test]
fn test_vendor_config_absent_file_returns_empty_config() {
    let tr = TestRepo::new();
    // No `.gitvendors` written — git2::Config::open succeeds but the config
    // is empty, so querying any key must fail.
    let cfg = tr.repo.vendor_config().unwrap();
    assert!(cfg.get_string("vendor.anything.url").is_err());
}


#[test]
fn test_vendor_config_fails_for_bare_repo() {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init_bare(dir.path()).unwrap();
    assert!(repo.vendor_config().is_err());
}

// ---------------------------------------------------------------------------
// list_vendors
// ---------------------------------------------------------------------------

#[test]
fn test_list_vendors_returns_all_entries() {
    let tr = TestRepo::new();
    tr.write_gitvendors(
        r#"
[vendor "alpha"]
    url = https://example.com/alpha.git
    branch = main

[vendor "beta"]
    url = https://example.com/beta.git
    base = cafebabe00000000000000000000000000000000
"#,
    );

    let mut vendors = tr.repo.list_vendors().unwrap();
    vendors.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(vendors.len(), 2);
    assert_eq!(vendors[0].name, "alpha");
    assert_eq!(vendors[0].branch.as_deref(), Some("main"));
    assert_eq!(vendors[1].name, "beta");
    assert_eq!(
        vendors[1].base.as_deref(),
        Some("cafebabe00000000000000000000000000000000")
    );
}

#[test]
fn test_list_vendors_empty_file_returns_empty_vec() {
    let tr = TestRepo::new();
    tr.write_gitvendors("");

    let vendors = tr.repo.list_vendors().unwrap();
    assert!(vendors.is_empty());
}

#[test]
fn test_list_vendors_fails_for_bare_repo() {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init_bare(dir.path()).unwrap();
    assert!(repo.list_vendors().is_err());
}

// ---------------------------------------------------------------------------
// get_vendor_by_name
// ---------------------------------------------------------------------------

#[test]
fn test_get_vendor_by_name_returns_correct_entry() {
    let tr = TestRepo::new();
    tr.write_gitvendors(
        r#"
[vendor "widgets"]
    url = https://example.com/widgets.git
    branch = stable
"#,
    );

    let vs = tr.repo.get_vendor_by_name("widgets").unwrap().unwrap();
    assert_eq!(vs.name, "widgets");
    assert_eq!(vs.url, "https://example.com/widgets.git");
    assert_eq!(vs.branch.as_deref(), Some("stable"));
    assert!(vs.base.is_none());
}

#[test]
fn test_get_vendor_by_name_returns_none_for_missing_entry() {
    let tr = TestRepo::new();
    tr.write_gitvendors(
        r#"
[vendor "widgets"]
    url = https://example.com/widgets.git
"#,
    );

    let result = tr.repo.get_vendor_by_name("nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_get_vendor_by_name_all_fields_preserved() {
    let tr = TestRepo::new();
    // Use a real-looking (but fake) SHA so from_config doesn't reject it.
    tr.write_gitvendors(
        r#"
[vendor "full"]
    url = https://example.com/full.git
    branch = develop
    base = 0000000000000000000000000000000000000001
"#,
    );

    let vs = tr.repo.get_vendor_by_name("full").unwrap().unwrap();
    assert_eq!(vs.url, "https://example.com/full.git");
    assert_eq!(vs.branch.as_deref(), Some("develop"));
    assert_eq!(
        vs.base.as_deref(),
        Some("0000000000000000000000000000000000000001")
    );
}

// ---------------------------------------------------------------------------
// find_vendor_base
// ---------------------------------------------------------------------------

#[test]
fn test_find_vendor_base_returns_none_when_no_base() {
    let tr = TestRepo::new();
    let vs = git_vendor::VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: None,
        path: None,
        patterns: vec![],
    };

    let result = tr.repo.find_vendor_base(&vs).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_find_vendor_base_returns_commit_when_base_exists() {
    let tr = TestRepo::new();

    // Create a real commit so we have a valid OID to reference.
    let tree_oid = tr.make_tree(&[("file.txt", b"hello")]);
    let commit_oid = tr.make_commit_oid(tree_oid, "base commit", &[]);

    let vs = git_vendor::VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: Some(commit_oid.to_string()),
        path: None,
        patterns: vec![],
    };

    let commit = tr.repo.find_vendor_base(&vs).unwrap().unwrap();
    assert_eq!(commit.id(), commit_oid);
}

#[test]
fn test_find_vendor_base_errors_on_invalid_oid() {
    let tr = TestRepo::new();
    let vs = git_vendor::VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: Some("not-a-valid-sha".into()),
        path: None,
        patterns: vec![],
    };

    assert!(tr.repo.find_vendor_base(&vs).is_err());
}

#[test]
fn test_find_vendor_base_errors_on_nonexistent_commit() {
    let tr = TestRepo::new();
    // A syntactically valid OID that doesn't exist in the repo.
    let vs = git_vendor::VendorSource {
        name: "foo".into(),
        url: "https://example.com/foo.git".into(),
        branch: None,
        base: Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into()),
        path: None,
        patterns: vec![],
    };

    assert!(tr.repo.find_vendor_base(&vs).is_err());
}

// ---------------------------------------------------------------------------
// check_vendors
// ---------------------------------------------------------------------------

#[test]
fn test_check_vendors_up_to_date_when_base_matches_head_ref() {
    let tr = TestRepo::new();

    // Create a commit and use its OID as both the vendor ref and the base.
    let tree_oid = tr.make_tree(&[("lib.rs", b"pub fn foo() {}")]);
    let commit_oid = tr.make_commit_oid(tree_oid, "vendor tip", &[]);
    tr.set_vendor_ref("mylib", commit_oid);

    tr.write_gitvendors(&format!(
        r#"
[vendor "mylib"]
    url = https://example.com/mylib.git
    base = {commit_oid}
"#,
    ));

    let updates = tr.repo.check_vendors().unwrap();
    assert_eq!(updates.len(), 1);

    let value = updates.values().next().unwrap();
    // base == head ref => no update needed.
    assert!(value.is_none());
}

#[test]
fn test_check_vendors_reports_new_tip_when_base_differs() {
    let tr = TestRepo::new();

    let old_tree = tr.make_tree(&[("lib.rs", b"v1")]);
    let old_oid = tr.make_commit_oid(old_tree, "old tip", &[]);

    let new_tree = tr.make_tree(&[("lib.rs", b"v2")]);
    let new_oid = tr.make_commit_oid(new_tree, "new tip", &[old_oid]);

    // Vendor ref points at the newer commit, but base records the older one.
    tr.set_vendor_ref("mylib", new_oid);

    tr.write_gitvendors(&format!(
        r#"
[vendor "mylib"]
    url = https://example.com/mylib.git
    base = {old_oid}
"#,
    ));

    let updates = tr.repo.check_vendors().unwrap();
    assert_eq!(updates.len(), 1);

    let value = updates.values().next().unwrap();
    // base != head ref => update available at new_oid.
    assert_eq!(*value, Some(new_oid));
}

#[test]
fn test_check_vendors_reports_tip_when_no_base() {
    let tr = TestRepo::new();

    let tree_oid = tr.make_tree(&[("lib.rs", b"pub fn bar() {}")]);
    let commit_oid = tr.make_commit_oid(tree_oid, "tip", &[]);
    tr.set_vendor_ref("mylib", commit_oid);

    // No `base` key — the vendor has never been merged.
    tr.write_gitvendors(
        r#"
[vendor "mylib"]
    url = https://example.com/mylib.git
"#,
    );

    let updates = tr.repo.check_vendors().unwrap();
    assert_eq!(updates.len(), 1);

    let value = updates.values().next().unwrap();
    assert_eq!(*value, Some(commit_oid));
}

#[test]
fn test_check_vendors_empty_config_returns_empty_map() {
    let tr = TestRepo::new();
    tr.write_gitvendors("");

    let updates = tr.repo.check_vendors().unwrap();
    assert!(updates.is_empty());
}

// ---------------------------------------------------------------------------
// merge_vendor — two-way (no base)
// ---------------------------------------------------------------------------

#[test]
fn test_merge_vendor_two_way_no_conflicts() {
    let tr = TestRepo::new();

    // Working tree: one file marked as vendored.
    tr.write_gitattributes("shared.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"shared.txt vendor=upstream\n"),
        ("shared.txt", b"ours content\n"),
    ]);

    // Vendor tree: same file with different content.
    let vendor_tree = tr.make_tree(&[("shared.txt", b"theirs content\n")]);
    let vendor_commit = tr.make_commit_oid(vendor_tree, "vendor tip", &[]);
    tr.set_vendor_ref("upstream", vendor_commit);

    tr.write_gitvendors(
        r#"
[vendor "upstream"]
    url = https://example.com/upstream.git
    pattern = **
"#,
    );

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    let index = tr.repo.merge_vendor(&vs, None, None).unwrap();

    // Two-way with no base: merge_trees(&theirs, &ours, &theirs) should
    // produce a valid index without hard conflicts on disjoint content.
    assert!(!index.has_conflicts());
}

#[test]
fn test_merge_vendor_two_way_empty_vendored_set() {
    let tr = TestRepo::new();

    // HEAD exists but nothing is marked vendored.
    tr.write_gitattributes("");
    tr.make_head_commit(&[(".gitattributes", b"")]);

    let vendor_tree = tr.make_tree(&[("upstream_file.txt", b"upstream\n")]);
    let vendor_commit = tr.make_commit_oid(vendor_tree, "vendor tip", &[]);
    tr.set_vendor_ref("upstream", vendor_commit);

    tr.write_gitvendors(
        r#"
[vendor "upstream"]
    url = https://example.com/upstream.git
    pattern = **
"#,
    );

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    // Should complete without error even when the vendored set is empty.
    let index = tr.repo.merge_vendor(&vs, None, None).unwrap();
    assert!(!index.has_conflicts());
}

// ---------------------------------------------------------------------------
// fetch_vendor
// ---------------------------------------------------------------------------

#[test]
fn test_fetch_vendor_creates_vendor_ref() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    let tip = remote.make_commit(&[("lib.rs", b"pub fn hello() {}")], "initial");

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    let reference = tr.repo.fetch_vendor(&vs, None).unwrap();

    assert_eq!(reference.name(), Some("refs/vendor/upstream/head"));
    assert_eq!(reference.target(), Some(tip));
}

#[test]
fn test_fetch_vendor_updates_ref_on_second_fetch() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    remote.make_commit(&[("lib.rs", b"v1")], "v1");
    let v2 = remote.make_commit(&[("lib.rs", b"v2")], "v2");

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    // First fetch — lands on v1 tip (actually both commits; ref points at v2
    // since that is HEAD at fetch time).
    tr.repo.fetch_vendor(&vs, None).unwrap();

    // Advance remote and fetch again.
    let v3 = remote.make_commit(&[("lib.rs", b"v3")], "v3");
    let reference = tr.repo.fetch_vendor(&vs, None).unwrap();

    assert_eq!(reference.target(), Some(v3));
    // v2 must still be reachable (history is intact).
    assert!(tr.repo.find_commit(v2).is_ok());
}

#[test]
fn test_fetch_vendor_explicit_branch() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    // Put a commit on a non-default branch.
    let tip = remote.make_commit_on(
        "refs/heads/stable",
        &[("lib.rs", b"stable content")],
        "stable release",
    );

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
    branch = refs/heads/stable
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    let reference = tr.repo.fetch_vendor(&vs, None).unwrap();

    assert_eq!(reference.target(), Some(tip));
}

#[test]
fn test_fetch_vendor_fails_on_bad_url() {
    let tr = TestRepo::new();
    let vs = git_vendor::VendorSource {
        name: "broken".into(),
        url: "file:///this/path/does/not/exist".into(),
        branch: None,
        base: None,
        path: None,
        patterns: vec![],
    };

    assert!(tr.repo.fetch_vendor(&vs, None).is_err());
}

// ---------------------------------------------------------------------------
// End-to-end: fetch → check → merge
// ---------------------------------------------------------------------------

#[test]
fn test_end_to_end_fetch_then_check_reports_update() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    let tip = remote.make_commit(&[("lib.rs", b"pub fn foo() {}")], "initial");

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    tr.repo.fetch_vendor(&vs, None).unwrap();

    // Make a HEAD commit so list_vendors / check_vendors can open vendor_config.
    tr.make_head_commit(&[(".gitattributes", b"")]);

    let updates = tr.repo.check_vendors().unwrap();
    assert_eq!(updates.len(), 1);
    let update = updates.values().next().unwrap();
    // No base has ever been recorded, so the tip is reported as a pending update.
    assert_eq!(*update, Some(tip));
}

#[test]
fn test_end_to_end_fetch_then_merge_two_way_clean() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    // Remote has a vendored file.
    remote.make_commit(&[("shared.txt", b"upstream content\n")], "initial");

    // Local HEAD has the same file marked vendored, with our local content.
    tr.write_gitattributes("shared.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"shared.txt vendor=upstream\n"),
        ("shared.txt", b"our content\n"),
    ]);

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
    pattern = **
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    tr.repo.fetch_vendor(&vs, None).unwrap();

    let index = tr.repo.merge_vendor(&vs, None, None).unwrap();
    assert!(!index.has_conflicts());
}

#[test]
fn test_end_to_end_fetch_then_merge_three_way_clean() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    // Establish the common base on the remote.
    let base_oid = remote.make_commit(&[("shared.txt", b"line one\n")], "base");

    // Local HEAD matches the base content.
    tr.write_gitattributes("shared.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"shared.txt vendor=upstream\n"),
        ("shared.txt", b"line one\n"),
    ]);

    // Remote advances independently.
    remote.make_commit(
        &[("shared.txt", b"line one\nline two\n")],
        "upstream advance",
    );

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
    base = {base_oid}
    pattern = **
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    tr.repo.fetch_vendor(&vs, None).unwrap();

    // Three-way merge: base → ours (unchanged) + base → theirs (added line).
    let index = tr.repo.merge_vendor(&vs, None, None).unwrap();
    assert!(!index.has_conflicts());
}

#[test]
fn test_end_to_end_fetch_then_merge_three_way_conflict_then_resolve() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    // Common ancestor.
    let base_oid = remote.make_commit(&[("shared.txt", b"original\n")], "base");

    // Local diverges from base.
    tr.write_gitattributes("shared.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"shared.txt vendor=upstream\n"),
        ("shared.txt", b"our change\n"),
    ]);

    // Remote also diverges from base — conflict guaranteed.
    remote.make_commit(&[("shared.txt", b"their change\n")], "upstream change");

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
    base = {base_oid}
    pattern = **
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    tr.repo.fetch_vendor(&vs, None).unwrap();

    let mut index = tr.repo.merge_vendor(&vs, None, None).unwrap();

    // The three-way merge must report a conflict on shared.txt.
    assert!(index.has_conflicts());

    // Collect conflicting paths.
    let conflicted_paths: Vec<String> = index
        .conflicts()
        .unwrap()
        .filter_map(|c| {
            let c = c.unwrap();
            c.our
                .or(c.their)
                .or(c.ancestor)
                .and_then(|e| String::from_utf8(e.path).ok())
        })
        .collect();
    assert!(
        conflicted_paths.iter().any(|p| p == "shared.txt"),
        "expected shared.txt in conflicts, got: {:?}",
        conflicted_paths
    );

    // Resolve: accept our version by writing a blob and clearing the conflict.
    let resolved = b"our change\n";
    let blob_oid = tr.repo.blob(resolved).unwrap();
    index.conflict_remove(Path::new("shared.txt")).unwrap();
    index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: resolved.len() as u32,
            id: blob_oid,
            flags: 0,
            flags_extended: 0,
            path: b"shared.txt".to_vec(),
        })
        .unwrap();

    assert!(!index.has_conflicts());

    // The resolved tree must contain shared.txt with our chosen content.
    let resolved_tree_oid = index.write_tree_to(&tr.repo).unwrap();
    let resolved_tree = tr.repo.find_tree(resolved_tree_oid).unwrap();
    let entry = resolved_tree.get_name("shared.txt").unwrap();
    let blob = tr.repo.find_blob(entry.id()).unwrap();
    assert_eq!(blob.content(), b"our change\n");
}

#[test]
fn test_end_to_end_fetch_then_merge_multiple_files_partial_conflict() {
    let tr = TestRepo::new();
    let remote = RemoteRepo::new();

    // Base: two vendored files.
    let base_oid = remote.make_commit(
        &[("api.txt", b"api v1\n"), ("readme.txt", b"readme v1\n")],
        "base",
    );

    // Locally we change api.txt but leave readme.txt alone.
    tr.write_gitattributes("*.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"*.txt vendor=upstream\n"),
        ("api.txt", b"our api change\n"),
        ("readme.txt", b"readme v1\n"),
    ]);

    // Remote changes api.txt (conflict) and also advances readme.txt (clean).
    remote.make_commit(
        &[
            ("api.txt", b"their api change\n"),
            ("readme.txt", b"readme v2\n"),
        ],
        "upstream changes",
    );

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = {url}
    base = {base_oid}
    pattern = **
"#,
        url = remote.url()
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    tr.repo.fetch_vendor(&vs, None).unwrap();

    let mut index = tr.repo.merge_vendor(&vs, None, None).unwrap();

    // api.txt must conflict; readme.txt must not.
    assert!(index.has_conflicts());
    let conflicted_paths: Vec<String> = index
        .conflicts()
        .unwrap()
        .filter_map(|c| {
            let c = c.unwrap();
            c.our
                .or(c.their)
                .or(c.ancestor)
                .and_then(|e| String::from_utf8(e.path).ok())
        })
        .collect();
    assert!(conflicted_paths.iter().any(|p| p == "api.txt"));
    assert!(!conflicted_paths.iter().any(|p| p == "readme.txt"));

    // Resolve api.txt by taking theirs.
    let resolved = b"their api change\n";
    let blob_oid = tr.repo.blob(resolved).unwrap();
    index.conflict_remove(Path::new("api.txt")).unwrap();
    index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: resolved.len() as u32,
            id: blob_oid,
            flags: 0,
            flags_extended: 0,
            path: b"api.txt".to_vec(),
        })
        .unwrap();

    assert!(!index.has_conflicts());

    let resolved_tree_oid = index.write_tree_to(&tr.repo).unwrap();
    let resolved_tree = tr.repo.find_tree(resolved_tree_oid).unwrap();

    let api_entry = resolved_tree.get_name("api.txt").unwrap();
    let api_blob = tr.repo.find_blob(api_entry.id()).unwrap();
    assert_eq!(api_blob.content(), b"their api change\n");

    let readme_entry = resolved_tree.get_name("readme.txt").unwrap();
    let readme_blob = tr.repo.find_blob(readme_entry.id()).unwrap();
    assert_eq!(readme_blob.content(), b"readme v2\n");
}

// ---------------------------------------------------------------------------
// merge_vendor — three-way (with base)
// ---------------------------------------------------------------------------

#[test]
fn test_merge_vendor_three_way_clean_merge() {
    let tr = TestRepo::new();

    // Base: the common ancestor content.
    let base_content = b"line one\nline two\n";
    tr.write_gitattributes("shared.txt vendor=upstream\n");
    tr.make_head_commit(&[
        (".gitattributes", b"shared.txt vendor=upstream\n"),
        ("shared.txt", base_content),
    ]);

    let base_tree = tr.make_tree(&[("shared.txt", base_content)]);
    let base_commit = tr.make_commit_oid(base_tree, "base", &[]);

    // Vendor advances the file; ours keeps it as-is (no conflicts expected).
    let vendor_tree = tr.make_tree(&[("shared.txt", b"line one\nline two\nline three\n")]);
    let vendor_commit = tr.make_commit_oid(vendor_tree, "vendor tip", &[base_commit]);
    tr.set_vendor_ref("upstream", vendor_commit);

    tr.write_gitvendors(&format!(
        r#"
[vendor "upstream"]
    url = https://example.com/upstream.git
    base = {base_commit}
    pattern = **
"#,
    ));

    let vs = tr.repo.get_vendor_by_name("upstream").unwrap().unwrap();
    let index = tr.repo.merge_vendor(&vs, None, None).unwrap();
    assert!(!index.has_conflicts());
}
