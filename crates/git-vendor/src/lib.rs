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
    /// Retrieve the top-level `.gitvendors` file as a `git2::Config` value.
    fn vendor_config(&self) -> Result<git2::Config, git2::Error>;

    /// Retrieve all vendored files in a given tree.
    fn vendored_subtree(&self) -> Result<git2::Tree<'_>, git2::Error>;

    /// Return all vendor sources tracked at the commit provided (defaulting to `HEAD`).
    fn list_vendors(&self) -> Result<Vec<VendorSource>, git2::Error>;

    /// Return all vendor sources mapped to the upstream tip OID if it differs from the base tree.
    /// `Some(oid)` means there are unmerged upstream changes at that commit; `None` means up to date.
    fn check_vendors(&self) -> Result<HashMap<VendorSource, Option<git2::Oid>>, git2::Error>;

    /// Track a vendor pattern by setting `vendor` and `vendor-prefix` attributes on matching files.
    fn track_vendor_pattern(
        &self,
        vendor: &VendorSource,
        glob: &str,
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
        glob: &str,
        path: &Path,
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

/// Checks if `repo` is bare, and if so, raises an error.
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
        let cfg = git2::Config::open(&workdir.join(".gitvendors"))?;
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
        glob: &str,
        path: &Path,
    ) -> Result<(), git2::Error> {
        let workdir = self
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
        let gitattributes = workdir.join(path).join(".gitattributes");
        let tree = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        // Normalize a trailing `/` (directory shorthand) to `dir/**` so that
        // globset matches all files under that directory recursively.
        let normalized: String;
        let pat = if glob.ends_with('/') {
            normalized = format!("{}**", glob);
            normalized.as_str()
        } else {
            glob
        };

        let mut glob_builder = globset::GlobSetBuilder::new();
        let g = globset::Glob::new(pat)
            .map_err(|e| git2::Error::from_str(&format!("Invalid pattern '{}': {}", glob, e)))?;
        glob_builder.add(g);
        let matcher = glob_builder
            .build()
            .map_err(|e| git2::Error::from_str(&e.to_string()))?;

        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                return git2::TreeWalkResult::Ok;
            }
            let remote_path = PathBuf::from(dir).join(entry.name().unwrap());
            if !matcher.is_match(&remote_path) {
                return git2::TreeWalkResult::Ok;
            }
            let prefix = remote_path.parent().unwrap_or(Path::new(""));
            let local_path = path.join(entry.name().unwrap());
            let vendor_attr = format!("vendor={}", vendor.name);
            let prefix_attr = format!("vendor-prefix={}", prefix.display());

            match self.set_attr(
                &local_path.to_string_lossy(),
                &[&vendor_attr, &prefix_attr],
                &gitattributes,
            ) {
                Ok(_) => return git2::TreeWalkResult::Ok,
                Err(_) => return git2::TreeWalkResult::Abort,
            };
        })?;

        Ok(())
    }

    fn add_vendor(
        &self,
        vendor: &VendorSource,
        glob: &str,
        _path: &Path,
    ) -> Result<git2::Index, git2::Error> {
        // Normalize a trailing `/` (directory shorthand) to `dir/**` so that
        // globset matches all files under that directory recursively.
        let normalized: String;
        let pat = if glob.ends_with('/') {
            normalized = format!("{}**", glob);
            normalized.as_str()
        } else {
            glob
        };

        let mut glob_builder = globset::GlobSetBuilder::new();
        let g = globset::Glob::new(pat)
            .map_err(|e| git2::Error::from_str(&format!("Invalid pattern '{}': {}", glob, e)))?;
        glob_builder.add(g);
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

        self.merge_trees(&empty_tree, &ours_filtered, &theirs_filtered, Some(&opts))
    }

    fn merge_vendor(
        &self,
        vendor: &VendorSource,
        _maybe_opts: Option<&mut git2::FetchOptions>,
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
