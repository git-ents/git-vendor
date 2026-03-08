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

/// Convert a path to a git-compatible string with forward slashes.
///
/// Git patterns (e.g. in `.gitattributes`) always use `/` as the separator,
/// but `Path::join` and `PathBuf::from` produce `\` on Windows.
fn to_git_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Build a [`globset::GlobSet`] from a slice of pattern strings, normalizing
/// trailing-`/` directory shorthands to `dir/**`.
fn build_glob_matcher(patterns: &[impl AsRef<str>]) -> Result<globset::GlobSet, git2::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pat in patterns {
        let pat = pat.as_ref();
        let normalized = if pat.ends_with('/') {
            format!("{}**", pat)
        } else {
            pat.to_string()
        };
        let g = globset::Glob::new(&normalized)
            .map_err(|e| git2::Error::from_str(&format!("Invalid pattern '{}': {}", pat, e)))?;
        builder.add(g);
    }
    builder
        .build()
        .map_err(|e| git2::Error::from_str(&e.to_string()))
}

/// All metadata required to retrieve necessary objects from a vendor.
#[derive(Clone, Hash, PartialEq, Eq)]
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
    /// Glob pattern(s) selecting which upstream files to vendor.
    pub patterns: Vec<String>,
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

        // Remove existing pattern entries before writing the current set.
        let pattern_key = format!("vendor.{}.pattern", &self.name);
        let _ = cfg.remove_multivar(&pattern_key, ".*");
        for pattern in &self.patterns {
            cfg.set_multivar(&pattern_key, "^$", pattern)?;
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

        let mut patterns = Vec::new();
        let pattern_entries = cfg.multivar(&format!("vendor.{name}.pattern"), None);
        if let Ok(pattern_entries) = pattern_entries {
            pattern_entries.for_each(|entry| {
                if let Some(value) = entry.value() {
                    patterns.push(value.to_string());
                }
            })?;
        }

        Ok(Some(Self {
            name,
            url,
            branch,
            base,
            patterns,
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

    /// Track vendor pattern(s) by writing per-file gitattributes lines with the `vendor` attribute.
    fn track_vendor_pattern(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
    ) -> Result<(), git2::Error>;

    /// Refresh `.gitattributes` after a merge so that per-file entries match
    /// the merged result.  New upstream files get entries; deleted files lose
    /// them.
    fn refresh_vendor_attrs(
        &self,
        vendor: &VendorSource,
        merged_index: &git2::Index,
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

        for glob in globs {
            let glob_patterns: Vec<String> = vec![glob.to_string()];
            let matcher = build_glob_matcher(&glob_patterns)?;

            let mut matched_files: Vec<String> = Vec::new();

            tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
                if entry.kind() != Some(git2::ObjectType::Blob) {
                    return git2::TreeWalkResult::Ok;
                }
                let remote_path = format!("{}{}", dir, entry.name().unwrap());
                if matcher.is_match(&remote_path) {
                    matched_files.push(remote_path);
                }
                git2::TreeWalkResult::Ok
            })?;

            if matched_files.is_empty() {
                continue;
            }

            let vendor_attr = format!("vendor={}", vendor.name);

            for file in &matched_files {
                let local_pattern = to_git_path(&path.join(file));
                self.set_attr(&local_pattern, &[&vendor_attr], &gitattributes)?;
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
        let matcher = build_glob_matcher(globs)?;

        // Build the set of upstream paths that match the glob pattern.
        let theirs = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_filtered =
            self.filter_by_predicate(&theirs, |_repo, entry_path| matcher.is_match(entry_path))?;

        // Collect upstream paths so we can filter HEAD to only overlapping
        // entries.  This lets merge_trees detect add/add conflicts when a
        // local file already exists at the same path as an incoming vendor
        // file.
        let mut upstream_paths: HashSet<String> = HashSet::new();
        theirs_filtered.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                upstream_paths.insert(format!("{}{}", dir, entry.name().unwrap()));
            }
            git2::TreeWalkResult::Ok
        })?;

        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered =
            self.filter_by_predicate(&ours, |_repo, p| upstream_paths.contains(&*to_git_path(p)))?;

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
        // UPSTREAM (theirs): use stored patterns to filter the upstream tree.
        // This catches files added upstream since the last merge.
        let matcher = build_glob_matcher(&vendor.patterns)?;
        let theirs = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_filtered =
            self.filter_by_predicate(&theirs, |_repo, path| matcher.is_match(path))?;

        // LOCAL (ours): use gitattributes to find files currently tracked for
        // this vendor.  Falls back to vendor patterns when the gitattribute
        // is unset (e.g. legacy .gitattributes with `./` prefixed patterns).
        let expected_vendor = vendor.name.clone();
        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered = self.filter_by_predicate(&ours, |repo, path| {
            match repo.get_attr(path, "vendor", git2::AttrCheckFlags::FILE_THEN_INDEX) {
                Ok(Some(value)) if value == expected_vendor => true,
                _ => matcher.is_match(path),
            }
        })?;

        let mut opts = git2::MergeOptions::new();
        opts.find_renames(true);
        opts.rename_threshold(50);
        if let Some(favor) = file_favor {
            opts.file_favor(favor);
        }

        let base_commit = self.find_vendor_base(&vendor)?;
        let base_full_tree;
        let base = match &base_commit {
            Some(c) => {
                base_full_tree = c.as_object().peel_to_tree()?;
                self.filter_by_predicate(&base_full_tree, |_repo, path| matcher.is_match(path))?
            }
            None => self.find_tree(ours_filtered.id())?,
        };

        self.merge_trees(&base, &ours_filtered, &theirs_filtered, Some(&opts))
    }

    fn refresh_vendor_attrs(
        &self,
        vendor: &VendorSource,
        merged_index: &git2::Index,
        path: &Path,
    ) -> Result<(), git2::Error> {
        let workdir = self
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
        let gitattributes = workdir.join(path).join(".gitattributes");
        let vendor_attr = format!("vendor={}", vendor.name);
        let matcher = build_glob_matcher(&vendor.patterns)?;

        // Collect all paths in the merged index that match the vendor's patterns.
        let mut merged_paths: HashSet<PathBuf> = HashSet::new();
        for entry in merged_index.iter() {
            let stage = (entry.flags >> 12) & 0x3;
            if stage != 0 {
                continue;
            }
            if let Ok(entry_path) = std::str::from_utf8(&entry.path) {
                let p = PathBuf::from(entry_path);
                if matcher.is_match(&p) {
                    merged_paths.insert(p);
                }
            }
        }

        // Read existing gitattributes, remove stale entries for this vendor,
        // keep everything else.
        let needle = format!("vendor={}", vendor.name);
        let mut lines: Vec<String> = if gitattributes.exists() {
            let content = std::fs::read_to_string(&gitattributes)
                .map_err(|e| git2::Error::from_str(&format!("read .gitattributes: {e}")))?;
            content
                .lines()
                .filter(|line| {
                    // Keep lines that don't belong to this vendor.
                    !line.split_whitespace().any(|tok| tok == needle)
                })
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };

        // Add per-file entries for all merged paths.
        let mut sorted: Vec<_> = merged_paths.into_iter().collect();
        sorted.sort();
        for file in sorted {
            let local_pattern = path.join(&file);
            let line = format!("{} {}", to_git_path(&local_pattern), vendor_attr);
            lines.push(line);
        }

        // Write back.
        if let Some(parent) = gitattributes.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                git2::Error::from_str(&format!("create dir for .gitattributes: {e}"))
            })?;
        }
        let mut content = lines.join("\n");
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        std::fs::write(&gitattributes, &content)
            .map_err(|e| git2::Error::from_str(&format!("write .gitattributes: {e}")))?;
        Ok(())
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
