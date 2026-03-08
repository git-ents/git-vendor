#![doc = include_str!("../README.md")]

pub mod cli;
pub mod exe;

use git_filter_tree::FilterTree;
use git_set_attr::SetAttr;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
};

use git2::Repository;

/// All metadata required to retrieve necessary objects from a vendor.
#[derive(Hash, PartialEq, Eq)]
pub struct VendorSource {
    /// The unique identifier for this particular vendor.
    pub name: String,
    pub url: String,
    /// The branch to track on the upstream remote.
    /// If not specified, this defaults to `HEAD`.
    pub branch: Option<String>,
    /// The most recent merge base. If not specified,
    /// it is assumed that no prior merge has taken
    /// place and conflicts must be resolved manually.
    pub base: Option<String>,
}

impl VendorSource {
    pub fn to_config(&self, cfg: &mut git2::Config) -> Result<(), git2::Error> {
        cfg.set_str(&format!("vendor.{}.url", &self.name), &self.url)?;

        if let Some(branch) = &self.branch {
            cfg.set_str(&format!("vendor.{}.branch", &self.name), branch)?;
        }

        if let Some(base) = &self.base {
            cfg.set_str(&format!("vendor.{}.base", &self.name), base)?;
        }

        Ok(())
    }

    pub fn from_config(cfg: &git2::Config, name: &str) -> Result<Option<Self>, git2::Error> {
        let name = name.to_string();
        let mut entries = cfg.entries(Some(&format!("vendor.{name}")))?;

        if entries.next().is_none() {
            return Ok(None);
        }

        let url = cfg.get_string(&format!("vendor.{name}.url"))?;
        let branch = cfg.get_string(&format!("vendor.{name}.branch")).ok();
        let base = cfg.get_string(&format!("vendor.{name}.base")).ok();

        Ok(Some(Self {
            name,
            url,
            branch,
            base,
        }))
    }

    /// The ref holding the latest fetched upstream tip.
    pub fn head_ref(&self) -> String {
        format!("refs/vendor/{}", self.name)
    }

    /// The ref to track.
    pub fn tracking_branch(&self) -> String {
        match &self.branch {
            Some(branch) => branch.clone(),
            None => "HEAD".into(),
        }
    }
}

fn vendors_from_config(cfg: &git2::Config) -> Result<Vec<VendorSource>, git2::Error> {
    let mut entries = cfg.entries(Some("vendor.*"))?;
    let mut vendor_names = std::collections::HashSet::new();

    while let Some(entry) = entries.next() {
        let entry = entry?;
        if let Some(name) = entry.name() {
            // Entry names look like "vendor.<name>.<key>"; extract <name>
            let parts: Vec<&str> = name.splitn(3, '.').collect();
            if parts.len() == 3 && parts[0] == "vendor" {
                vendor_names.insert(parts[1].to_string());
            }
        }
    }

    let mut vendors = Vec::new();
    for name in vendor_names {
        let vendor = VendorSource::from_config(cfg, &name)?;
        if let Some(vendor) = vendor {
            vendors.push(vendor);
        } else {
            return Err(git2::Error::from_str("vendor not found"));
        }
    }

    Ok(vendors)
}

/// A trait which provides methods for vendoring content across repository boundaries.
pub trait Vendor {
    /// Retrieve vendor configuration by merging three levels (lowest → highest
    /// priority), analogous to `git config`:
    ///
    /// 1. **Global** – `~/.gitvendors`
    /// 2. **Local**  – `$GIT_DIR/gitvendors`
    /// 3. **Index**  – `$WORKDIR/.gitvendors` (tracked)
    ///
    /// Writes go to the highest-priority file present in the stack (index).
    fn vendor_config(&self) -> Result<git2::Config, git2::Error>;

    /// Retrieve all vendored files in a given tree.
    fn vendored_subtree(&self) -> Result<git2::Tree<'_>, git2::Error>;

    /// Return all vendor sources tracked at the commit provided (defaulting to `HEAD`).
    fn list_vendors(&self) -> Result<Vec<VendorSource>, git2::Error>;

    /// Return all vendor sources mapped to the upstream tip OID if it differs from the base tree.
    /// `Some(oid)` means there are unmerged upstream changes at that commit; `None` means up to date.
    fn check_vendors(&self) -> Result<HashMap<VendorSource, Option<git2::Oid>>, git2::Error>;

    /// Track vendor pattern(s) by writing per-prefix gitattributes lines with `vendor` and `vendor-prefix` attributes.
    fn track_vendor_pattern(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
    ) -> Result<(), git2::Error>;

    /// Fetch the upstream for the given vendor and advance `refs/vendor/$name`.
    /// Returns the updated reference.
    fn fetch_vendor<'a>(
        &'a self,
        source: &VendorSource,
        maybe_opts: Option<&mut git2::FetchOptions>,
    ) -> Result<git2::Reference<'a>, git2::Error>;

    /// Perform the initial add of a vendor source.
    ///
    /// Unlike `merge_vendor`, which relies on files already present in HEAD to
    /// determine the upstream ↔ local mapping, `add_vendor` uses the given
    /// `glob` and `path` to filter the upstream tree directly.  This makes it
    /// suitable for the first-time add where no vendor files exist in HEAD yet.
    ///
    /// The resulting `git2::Index` contains the merged entries ready to be
    /// written to the working tree and staged.
    fn add_vendor(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error>;

    /// If a `base` exists in the vendor source provided (by `name`),
    /// initiate a three-way merge with the base reference, the
    /// commit provided (defaulting to the repository's `HEAD`),
    /// and the tip of `refs/vendor/{name}`. If no `base` exists,
    /// then a two-way merge is performed and a new `base` is written
    /// to the the returned `VendorSource`.
    fn merge_vendor(
        &self,
        vendor: &VendorSource,
        maybe_opts: Option<&mut git2::FetchOptions>,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error>;

    /// Given a vendor's name and a target commit (defaulting to `HEAD`),
    /// return the vendor's `base` reference it it exists. If no such `base`
    /// exists for the provided vendor source, `None` is returned.
    fn find_vendor_base(
        &self,
        vendor: &VendorSource,
    ) -> Result<Option<git2::Commit<'_>>, git2::Error>;

    /// Return a `VendorSource` which matches the provided name, if one exists
    /// in the provided `commit` (defaulting to `HEAD`).
    fn get_vendor_by_name(&self, name: &str) -> Result<Option<VendorSource>, git2::Error>;
}

fn bail_if_bare(repo: &Repository) -> Result<(), git2::Error> {
    // TODO: add support for bare repositories
    // Support for bare repositories is currently blocked by the lack of
    // in-memory `gitconfig` readers. How hard can that be to make?
    if repo.is_bare() {
        return Err(git2::Error::from_str(
            "a working tree is required; bare repositories are not supported",
        ));
    }

    Ok(())
}

impl Vendor for Repository {
    fn vendor_config(&self) -> Result<git2::Config, git2::Error> {
        bail_if_bare(self)?;
        let workdir = self
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;

        let mut cfg = git2::Config::new()?;

        // Global ~/.gitvendors (lowest priority).
        // Derive the home directory from libgit2's own global config path
        // (~/.gitconfig) so we don't depend on env vars directly.
        if let Some(global_path) = git2::Config::find_global()
            .ok()
            .and_then(|p| p.parent().map(|h| h.join(".gitvendors")))
            .filter(|p| p.exists())
        {
            cfg.add_file(&global_path, git2::ConfigLevel::Global, false)?;
        }

        // Local $GIT_DIR/gitvendors (repo-private, not tracked).
        let local_path = self.path().join("gitvendors");
        if local_path.exists() {
            cfg.add_file(&local_path, git2::ConfigLevel::Local, false)?;
        }

        // Index $WORKDIR/.gitvendors (tracked, highest priority).
        let index_path = workdir.join(".gitvendors");
        cfg.add_file(&index_path, git2::ConfigLevel::App, false)?;

        Ok(cfg)
    }

    fn vendored_subtree(&self) -> Result<git2::Tree<'_>, git2::Error> {
        let head = self.head()?.peel_to_tree()?;

        let mut vendored_entries: Vec<git2::TreeEntry> = Vec::new();

        head.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
            if let Some(attrs) = entry.name().and_then(|name| {
                self.get_attr(
                    &PathBuf::from_str(name).ok()?,
                    "vendored",
                    git2::AttrCheckFlags::FILE_THEN_INDEX,
                )
                .ok()
            }) {
                if attrs == Some("true") || attrs == Some("set") {
                    vendored_entries.push(entry.to_owned());
                }
            }
            git2::TreeWalkResult::Ok
        })?;

        todo!()
    }

    fn list_vendors(&self) -> Result<Vec<VendorSource>, git2::Error> {
        let cfg = self.vendor_config()?;
        vendors_from_config(&cfg)
    }

    fn fetch_vendor<'a>(
        &'a self,
        vendor: &VendorSource,
        maybe_opts: Option<&mut git2::FetchOptions>,
    ) -> Result<git2::Reference<'a>, git2::Error> {
        let mut remote = self.remote_anonymous(&vendor.url)?;
        let refspec = format!("{}:{}", vendor.tracking_branch(), vendor.head_ref());
        remote.fetch(&[&refspec], maybe_opts, None)?;

        let head = self.find_reference(&vendor.head_ref())?;

        Ok(head)
    }

    fn check_vendors(&self) -> Result<HashMap<VendorSource, Option<git2::Oid>>, git2::Error> {
        let vendors = self.list_vendors()?;
        let mut updates = HashMap::new();

        for vendor in vendors {
            match vendor.base.as_ref() {
                Some(base) => {
                    let base = git2::Oid::from_str(base)?;
                    let head = self.find_reference(&vendor.head_ref())?.target().ok_or(
                        git2::Error::from_str("head ref was not found; this is an internal error"),
                    )?;

                    if base == head {
                        updates.insert(vendor, None);
                    } else {
                        updates.insert(vendor, Some(head));
                    }
                }
                None => {
                    let head = self.find_reference(&vendor.head_ref())?.target().ok_or(
                        git2::Error::from_str("head ref was not found; this is an internal error"),
                    )?;
                    updates.insert(vendor, Some(head));
                }
            }
        }

        Ok(updates)
    }

    fn track_vendor_pattern(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
    ) -> Result<(), git2::Error> {
        let workdir = self
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
        let gitattributes = workdir.join(path).join(".gitattributes");
        let tree = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;

        // For each user-provided glob, collect the unique upstream prefixes
        // (directory components) where matching files reside, then write one
        // gitattributes line per (glob, prefix) pair instead of one per file.
        for glob in globs {
            let normalized = if glob.ends_with('/') {
                format!("{}**", glob)
            } else {
                glob.to_string()
            };
            let compiled = globset::Glob::new(&normalized).map_err(|e| {
                git2::Error::from_str(&format!("Invalid pattern '{}': {}", glob, e))
            })?;
            let matcher = compiled.compile_matcher();

            let mut prefixes: HashSet<PathBuf> = HashSet::new();

            tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
                if entry.kind() != Some(git2::ObjectType::Blob) {
                    return git2::TreeWalkResult::Ok;
                }
                let remote_path = PathBuf::from(dir).join(entry.name().unwrap());
                if matcher.is_match(&remote_path) {
                    let prefix = remote_path.parent().unwrap_or(Path::new(""));
                    prefixes.insert(prefix.to_path_buf());
                }
                git2::TreeWalkResult::Ok
            })?;

            let vendor_attr = format!("vendor={}", vendor.name);

            for prefix in &prefixes {
                // Build the local pattern: path + user glob (filename portion).
                // For a glob like `**/*.c` matching prefix `lib`, the
                // gitattributes pattern becomes `third_party/*.c` (the local
                // path joined with the glob's filename component).
                //
                // Directory globs (`sub/`) expand to `path/sub/**` so the
                // gitattributes pattern reflects the actual local directory
                // where vendored files reside.
                let local_pattern = if glob.ends_with('/') {
                    let dir = glob.trim_end_matches('/');
                    path.join(dir).join("**")
                } else {
                    let glob_filename = Path::new(glob)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| glob.to_string());
                    path.join(&glob_filename)
                };
                let prefix_attr = format!("vendor-prefix={}", prefix.display());

                self.set_attr(
                    &local_pattern.to_string_lossy(),
                    &[&vendor_attr, &prefix_attr],
                    &gitattributes,
                )?;
            }
        }

        Ok(())
    }

    fn add_vendor(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        _path: &Path,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error> {
        let mut glob_builder = globset::GlobSetBuilder::new();
        for glob in globs {
            // Normalize a trailing `/` (directory shorthand) to `dir/**` so that
            // globset matches all files under that directory recursively.
            let pat = if glob.ends_with('/') {
                format!("{}**", glob)
            } else {
                glob.to_string()
            };
            let g = globset::Glob::new(&pat).map_err(|e| {
                git2::Error::from_str(&format!("Invalid pattern '{}': {}", glob, e))
            })?;
            glob_builder.add(g);
        }
        let matcher = glob_builder
            .build()
            .map_err(|e| git2::Error::from_str(&e.to_string()))?;

        // Build the set of upstream paths that match the glob pattern.
        let theirs = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_filtered =
            self.filter_by_predicate(&theirs, |_repo, entry_path| matcher.is_match(entry_path))?;

        // Collect upstream paths so we can filter HEAD to only overlapping
        // entries.  This lets merge_trees detect add/add conflicts when a
        // local file already exists at the same path as an incoming vendor
        // file.
        let mut upstream_paths: HashSet<PathBuf> = HashSet::new();
        theirs_filtered.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                upstream_paths.insert(PathBuf::from(dir).join(entry.name().unwrap()));
            }
            git2::TreeWalkResult::Ok
        })?;

        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered =
            self.filter_by_predicate(&ours, |_repo, p| upstream_paths.contains(&PathBuf::from(p)))?;

        // Two-way merge: empty ancestor so that both sides look like pure
        // additions.  If the same path exists in both ours and theirs with
        // different content, git2 will report an add/add conflict.
        let empty_tree = {
            let empty_oid = self.treebuilder(None)?.write()?;
            self.find_tree(empty_oid)?
        };

        let mut opts = git2::MergeOptions::new();
        opts.find_renames(true);
        opts.rename_threshold(50);
        if let Some(favor) = file_favor {
            opts.file_favor(favor);
        }

        self.merge_trees(&empty_tree, &ours_filtered, &theirs_filtered, Some(&opts))
    }

    fn merge_vendor(
        &self,
        vendor: &VendorSource,
        _maybe_opts: Option<&mut git2::FetchOptions>,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error> {
        let expected_vendor = vendor.name.clone();
        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered = self.filter_by_predicate(&ours, |repo, path| {
            match repo.get_attr(path, "vendor", git2::AttrCheckFlags::FILE_THEN_INDEX) {
                Ok(Some(value)) => value == expected_vendor,
                _ => false,
            }
        })?;

        let mut expected_remote: HashSet<PathBuf> = HashSet::new();
        ours_filtered.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                return git2::TreeWalkResult::Ok;
            }
            let local_path = PathBuf::from(dir).join(entry.name().unwrap());
            let prefix = self
                .get_attr(
                    &local_path,
                    "vendor-prefix",
                    git2::AttrCheckFlags::FILE_THEN_INDEX,
                )
                .ok()
                .flatten()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::new());
            expected_remote.insert(prefix.join(local_path.file_name().unwrap()));
            git2::TreeWalkResult::Ok
        })?;

        let theirs = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_filtered = self.filter_by_predicate(&theirs, |_repo, path| {
            expected_remote.contains(&PathBuf::from(path))
        })?;

        let mut opts = git2::MergeOptions::new();
        opts.find_renames(true);
        opts.rename_threshold(50);
        if let Some(favor) = file_favor {
            opts.file_favor(favor);
        }

        let base = match self.find_vendor_base(&vendor)? {
            Some(c) => c.as_object().peel_to_tree()?,
            None => self.find_tree(ours_filtered.id())?,
        };

        self.merge_trees(&base, &ours_filtered, &theirs_filtered, Some(&opts))
    }

    fn find_vendor_base(
        &self,
        vendor: &VendorSource,
    ) -> Result<Option<git2::Commit<'_>>, git2::Error> {
        match vendor.base.as_ref() {
            Some(base) => {
                let oid = git2::Oid::from_str(base)?;
                let commit = self.find_commit(oid)?;
                return Ok(Some(commit));
            }
            _ => Ok(None),
        }
    }

    fn get_vendor_by_name(&self, name: &str) -> Result<Option<VendorSource>, git2::Error> {
        let gitvendors = self.vendor_config()?;
        VendorSource::from_config(&gitvendors, name)
    }
}

#[cfg(test)]
mod tests;
