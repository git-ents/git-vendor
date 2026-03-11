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

/// A parsed pattern entry with an optional destination prefix.
///
/// The raw config value uses the syntax `<glob>` or `<glob>:<destination>`.
/// For example:
/// - `src/**` – match `src/**`, no remapping (files keep their upstream path)
/// - `src/**:ext/` – match `src/**`, strip the literal prefix `src/`, then
///   prepend `ext/` to get the local path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternMapping {
    /// The glob string (left of the colon, or the whole value when no colon).
    pub glob: String,
    /// The local destination prefix (right of the colon), if present.
    pub destination: Option<String>,
}

impl PatternMapping {
    /// Parse a raw pattern string, splitting on the first `:` only.
    pub fn parse(raw: &str) -> Self {
        match raw.split_once(':') {
            Some((glob, dest)) => PatternMapping {
                glob: glob.to_string(),
                destination: if dest.is_empty() {
                    None
                } else {
                    Some(dest.to_string())
                },
            },
            None => PatternMapping {
                glob: raw.to_string(),
                destination: None,
            },
        }
    }

    /// Serialize back to the raw config string.
    pub fn to_raw(&self) -> String {
        match &self.destination {
            Some(dest) => format!("{}:{}", self.glob, dest),
            None => self.glob.clone(),
        }
    }

    /// Extract the literal (non-glob) leading path component(s) from the glob.
    ///
    /// "Literal prefix" is everything before the first glob character (`*`, `?`,
    /// `[`).  For `src/**/*.rs` this returns `src/`.  For `**` it returns `""`.
    pub fn literal_prefix(&self) -> &str {
        let glob = self.glob.as_str();
        // Normalize trailing '/' patterns (e.g. "src/") – the literal prefix
        // is the whole string in that case.
        let first_glob = glob.find(['*', '?', '[']);
        match first_glob {
            Some(0) => "",
            Some(idx) => &glob[..idx],
            None => {
                // No glob characters at all; the whole thing is a literal prefix
                // (directory shorthand).
                if glob.ends_with('/') { glob } else { "" }
            }
        }
    }

    /// Compute the local path for an upstream file that matched this pattern.
    ///
    /// 1. Strip the literal prefix from `upstream_path`.
    /// 2. If a `destination` is set, prepend it.
    ///
    /// Returns `None` if the upstream path doesn't start with the literal
    /// prefix (which shouldn't happen when the glob matched, but we guard
    /// defensively).
    pub fn local_path(&self, upstream_path: &str) -> Option<String> {
        let prefix = self.literal_prefix();
        let stripped = if prefix.is_empty() {
            upstream_path
        } else {
            upstream_path.strip_prefix(prefix)?
        };
        Some(match &self.destination {
            Some(dest) => {
                // Ensure dest ends with '/' when non-empty so paths join correctly.
                let dest = dest.trim_end_matches('/');
                if dest.is_empty() {
                    stripped.to_string()
                } else {
                    format!("{}/{}", dest, stripped)
                }
            }
            None => upstream_path.to_string(),
        })
    }
}

/// Parse a slice of raw pattern strings into [`PatternMapping`]s.
pub fn parse_patterns(raws: &[impl AsRef<str>]) -> Vec<PatternMapping> {
    raws.iter()
        .map(|r| PatternMapping::parse(r.as_ref()))
        .collect()
}

/// Build a [`globset::GlobSet`] from a slice of [`PatternMapping`]s, using
/// only the glob side (left of `:`).
fn build_glob_matcher_from_mappings(
    mappings: &[PatternMapping],
) -> Result<globset::GlobSet, git2::Error> {
    let globs: Vec<&str> = mappings.iter().map(|m| m.glob.as_str()).collect();
    build_glob_matcher(&globs)
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

/// Convert a `(globs, path)` pair — the legacy API shape — into a
/// `Vec<PatternMapping>`.
///
/// Each glob is parsed with [`PatternMapping::parse`].  If a glob already
/// carries an explicit colon destination, that destination is used as-is.
/// Otherwise `path` is applied as the destination prefix (unless `path` is
/// `"."` or empty, which means "no remapping").
fn globs_and_path_to_mappings(globs: &[&str], path: &Path) -> Vec<PatternMapping> {
    // Determine the normalized destination string from `path`.
    let dest: Option<String> = {
        let s = path.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/');
        if s.is_empty() || s == "." {
            None
        } else {
            Some(format!("{}/", s))
        }
    };

    globs
        .iter()
        .map(|raw| {
            let m = PatternMapping::parse(raw);
            if m.destination.is_some() {
                // Already has an explicit colon mapping – keep it.
                m
            } else if let Some(ref d) = dest {
                // Apply `path` as the destination.
                PatternMapping {
                    glob: m.glob,
                    destination: Some(d.clone()),
                }
            } else {
                m
            }
        })
        .collect()
}

/// Find the first [`PatternMapping`] from `mappings` whose glob matches
/// `upstream_path`, and return the computed local path.
///
/// Returns `None` if no pattern matches.
fn apply_pattern_mappings(mappings: &[PatternMapping], upstream_path: &str) -> Option<String> {
    for mapping in mappings {
        let glob = if mapping.glob.ends_with('/') {
            format!("{}**", mapping.glob)
        } else {
            mapping.glob.clone()
        };
        let g = globset::Glob::new(&glob).ok()?;
        let matcher = globset::GlobSetBuilder::new().add_then_build(g).ok()?;
        if matcher.is_match(upstream_path) {
            return mapping.local_path(upstream_path);
        }
    }
    None
}

/// Extension trait for [`globset::GlobSetBuilder`] to support chaining.
trait GlobSetBuilderExt {
    fn add_then_build(self, glob: globset::Glob) -> Result<globset::GlobSet, globset::Error>;
}

impl GlobSetBuilderExt for globset::GlobSetBuilder {
    fn add_then_build(mut self, glob: globset::Glob) -> Result<globset::GlobSet, globset::Error> {
        self.add(glob);
        self.build()
    }
}

/// Build a new git tree in `repo` containing only the upstream files that
/// match one of `mappings`, placed at their **local** (remapped) paths.
///
/// For each blob in `upstream_tree`, the first matching [`PatternMapping`] is
/// used to compute the local path.  Files that match no pattern are skipped.
///
/// The resulting tree has entries keyed by local path, ready to be used as
/// "theirs" in a merge against HEAD.
fn remap_upstream_tree<'a>(
    repo: &'a git2::Repository,
    upstream_tree: &git2::Tree<'_>,
    mappings: &[PatternMapping],
) -> Result<git2::Tree<'a>, git2::Error> {
    // Collect (local_path, blob_oid, mode) pairs.
    let mut entries: Vec<(String, git2::Oid, u32)> = Vec::new();

    upstream_tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return git2::TreeWalkResult::Ok;
        }
        let upstream_path = format!("{}{}", dir, entry.name().unwrap_or(""));
        if let Some(local_path) = apply_pattern_mappings(mappings, &upstream_path) {
            entries.push((local_path, entry.id(), entry.filemode() as u32));
        }
        git2::TreeWalkResult::Ok
    })?;

    build_tree_from_entries(repo, &entries)
}

/// Build a git tree from a flat list of `(path, blob_oid, mode)` entries,
/// creating nested subtrees as needed.
fn build_tree_from_entries<'a>(
    repo: &'a git2::Repository,
    entries: &[(String, git2::Oid, u32)],
) -> Result<git2::Tree<'a>, git2::Error> {
    // Group entries by their top-level component, then recurse.
    // Entries at the root are inserted directly; others go into subtrees.
    let mut root_files: Vec<(&str, git2::Oid, u32)> = Vec::new();
    let mut subdirs: std::collections::BTreeMap<&str, Vec<(String, git2::Oid, u32)>> =
        std::collections::BTreeMap::new();

    for (path, oid, mode) in entries {
        if let Some((dir, rest)) = path.split_once('/') {
            subdirs
                .entry(dir)
                .or_default()
                .push((rest.to_string(), *oid, *mode));
        } else {
            root_files.push((path.as_str(), *oid, *mode));
        }
    }

    let mut builder = repo.treebuilder(None)?;
    for (name, oid, mode) in root_files {
        builder.insert(name, oid, mode as i32)?;
    }
    for (dir, sub_entries) in &subdirs {
        let subtree = build_tree_from_entries(repo, sub_entries)?;
        builder.insert(dir, subtree.id(), 0o040000)?;
    }

    let oid = builder.write()?;
    repo.find_tree(oid)
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
    ///
    /// `globs` selects which upstream files to track.  `path` is the local
    /// directory under which the vendored files will live; it acts as the
    /// destination prefix for each glob.  Use `Path::new(".")` when the files
    /// are placed at the repository root.
    ///
    /// Patterns that already carry an explicit colon mapping (e.g. `src/**:ext/`)
    /// are stored as-is in [`VendorSource::patterns`] and are interpreted by
    /// the internal [`PatternMapping`] machinery when `path` is `"."`.
    fn track_vendor_pattern(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
    ) -> Result<(), git2::Error>;

    /// Refresh `.gitattributes` after a merge so that per-file entries match
    /// the merged result.  New upstream files get entries; deleted files lose
    /// them.
    ///
    /// `path` is the local directory under which vendored files live.  Pass
    /// `Path::new(".")` when files are at the repository root.
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
    /// `globs` and `path` to filter and place the upstream tree directly.  This
    /// makes it suitable for the first-time add where no vendor files exist in
    /// HEAD yet.
    ///
    /// `globs` may contain plain glob strings or strings with the colon mapping
    /// syntax (e.g. `src/**:ext/`); see [`PatternMapping`].  When a plain glob
    /// is given, `path` acts as the destination prefix.
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
            }) && (attrs == Some("true") || attrs == Some("set"))
            {
                vendored_entries.push(entry.to_owned());
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
        // Always write to the root .gitattributes.
        let gitattributes = workdir.join(".gitattributes");
        let tree = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let vendor_attr = format!("vendor={}", vendor.name);

        // Convert (globs, path) to PatternMappings.
        // Each glob may already carry colon syntax (e.g. "src/**:ext/"); if so,
        // parse it directly.  Otherwise apply `path` as the destination prefix.
        let mappings = globs_and_path_to_mappings(globs, path);

        // Collect (local_path) for each upstream file matched by any mapping.
        let mut matched_local_paths: Vec<String> = Vec::new();

        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                return git2::TreeWalkResult::Ok;
            }
            let upstream_path = format!("{}{}", dir, entry.name().unwrap_or(""));
            if let Some(local_path) = apply_pattern_mappings(&mappings, &upstream_path) {
                matched_local_paths.push(local_path);
            }
            git2::TreeWalkResult::Ok
        })?;

        for local_path in &matched_local_paths {
            self.set_attr(local_path, &[&vendor_attr], &gitattributes)?;
        }

        Ok(())
    }

    fn add_vendor(
        &self,
        vendor: &VendorSource,
        globs: &[&str],
        path: &Path,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error> {
        // Convert (globs, path) to PatternMappings.
        let mappings = globs_and_path_to_mappings(globs, path);

        // Build the remapped upstream tree: each upstream file is placed at its
        // local (mapped) path according to the pattern mappings.
        let upstream_tree = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_remapped = remap_upstream_tree(self, &upstream_tree, &mappings)?;

        // Collect local paths so we can filter HEAD to only overlapping entries.
        // This lets merge_trees detect add/add conflicts when a local file already
        // exists at the same local path as an incoming vendor file.
        let mut local_paths: HashSet<String> = HashSet::new();
        theirs_remapped.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                local_paths.insert(format!("{}{}", dir, entry.name().unwrap_or("")));
            }
            git2::TreeWalkResult::Ok
        })?;

        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered =
            self.filter_by_predicate(&ours, |_repo, p| local_paths.contains(&*to_git_path(p)))?;

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

        self.merge_trees(&empty_tree, &ours_filtered, &theirs_remapped, Some(&opts))
    }

    fn merge_vendor(
        &self,
        vendor: &VendorSource,
        _maybe_opts: Option<&mut git2::FetchOptions>,
        file_favor: Option<git2::FileFavor>,
    ) -> Result<git2::Index, git2::Error> {
        // Parse stored patterns into mappings (supports colon syntax).
        let mappings = parse_patterns(&vendor.patterns);

        // UPSTREAM (theirs): remap the upstream tree to local paths via mappings.
        let upstream_tree = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let theirs_remapped = remap_upstream_tree(self, &upstream_tree, &mappings)?;

        // LOCAL (ours): use gitattributes to find files currently tracked for
        // this vendor.  Falls back to glob matching against local paths when the
        // gitattribute is unset (e.g. legacy .gitattributes or first-ever merge).
        let glob_matcher = build_glob_matcher_from_mappings(&mappings)?;
        let expected_vendor = vendor.name.clone();
        let ours = self.head()?.peel_to_tree()?;
        let ours_filtered = self.filter_by_predicate(&ours, |repo, path| {
            match repo.get_attr(path, "vendor", git2::AttrCheckFlags::FILE_THEN_INDEX) {
                Ok(Some(value)) if value == expected_vendor => true,
                // Legacy fallback: match local paths against the glob side of patterns.
                _ => glob_matcher.is_match(path),
            }
        })?;

        let mut opts = git2::MergeOptions::new();
        opts.find_renames(true);
        opts.rename_threshold(50);
        if let Some(favor) = file_favor {
            opts.file_favor(favor);
        }

        // BASE: if a base commit is recorded, remap its tree the same way.
        let base_commit = self.find_vendor_base(vendor)?;
        let base = match &base_commit {
            Some(c) => {
                let base_full_tree = c.as_object().peel_to_tree()?;
                remap_upstream_tree(self, &base_full_tree, &mappings)?
            }
            None => self.find_tree(ours_filtered.id())?,
        };

        self.merge_trees(&base, &ours_filtered, &theirs_remapped, Some(&opts))
    }

    fn refresh_vendor_attrs(
        &self,
        vendor: &VendorSource,
        merged_index: &git2::Index,
        _path: &Path,
    ) -> Result<(), git2::Error> {
        let workdir = self
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
        // Always use the root .gitattributes regardless of `_path`.
        // The `_path` parameter is retained for API compatibility; the actual
        // per-file paths are taken directly from the merged index entries.
        let gitattributes = workdir.join(".gitattributes");
        let vendor_attr = format!("vendor={}", vendor.name);

        // Collect all stage-0 paths from the merged index that belong to this
        // vendor.  We identify them by the `vendor` gitattribute (or by
        // checking if the path appears in the remapped tree when no attr yet).
        let expected_vendor = vendor.name.clone();
        let mut merged_paths: HashSet<String> = HashSet::new();
        for entry in merged_index.iter() {
            let stage = (entry.flags >> 12) & 0x3;
            if stage != 0 {
                continue;
            }
            if let Ok(entry_path) = std::str::from_utf8(&entry.path) {
                // Include paths already attributed to this vendor in HEAD,
                // plus any new path that was brought in by the merge
                // (detected by checking the gitattr on the merged result –
                // which may not yet be set for brand-new files, so we
                // include all stage-0 entries and let the attribute file
                // be the source of truth going forward).
                merged_paths.insert(entry_path.to_string());
            }
        }

        // Filter to only paths that are (or should be) owned by this vendor.
        // Strategy: a path belongs to this vendor if:
        //   (a) it already carries `vendor=<name>` in HEAD's gitattributes, OR
        //   (b) it is a new file not yet attributed (we conservatively include
        //       all stage-0 entries from the merge result and trust that the
        //       caller only invokes refresh_vendor_attrs with a properly-scoped
        //       merged_index).
        let owned_paths: HashSet<String> = merged_paths
            .into_iter()
            .filter(|path| {
                match self.get_attr(
                    Path::new(path),
                    "vendor",
                    git2::AttrCheckFlags::FILE_THEN_INDEX,
                ) {
                    Ok(Some(value)) => value == expected_vendor,
                    // New file not yet attributed – include it.
                    _ => true,
                }
            })
            .collect();

        // Read existing root .gitattributes, remove stale entries for this
        // vendor, keep everything else.
        let needle = format!("vendor={}", vendor.name);
        let mut lines: Vec<String> = if gitattributes.exists() {
            let content = std::fs::read_to_string(&gitattributes)
                .map_err(|e| git2::Error::from_str(&format!("read .gitattributes: {e}")))?;
            content
                .lines()
                .filter(|line| !line.split_whitespace().any(|tok| tok == needle))
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };

        // Add per-file entries for all merged paths.
        let mut sorted: Vec<_> = owned_paths.into_iter().collect();
        sorted.sort();
        for file in sorted {
            let line = format!("{} {}", to_git_path(Path::new(&file)), vendor_attr);
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
                Ok(Some(commit))
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
