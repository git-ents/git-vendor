use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use git_set_attr::SetAttr;
use git2::Repository;

use crate::CommitMode;
use crate::PatternMapping;
use crate::Vendor;
use crate::VendorSource;

use crate::parse_patterns;
use crate::remap_upstream_tree;

/// Open a repository from the given path, or from the environment / current
/// directory when `None` is passed.
pub fn open_repo(path: Option<&Path>) -> Result<Repository, git2::Error> {
    match path {
        Some(p) => Repository::open(p),
        None => Repository::open_from_env(),
    }
}

/// List every vendor source configured in the repository.
///
/// Returns a `Vec` of `VendorSource` entries.  An empty `Vec` means no
/// vendors are configured.
pub fn list(repo: &Repository) -> Result<Vec<VendorSource>, git2::Error> {
    repo.list_vendors()
}

/// Register a new vendor source, fetch its upstream, merge the vendor tree
/// into the working directory, and stage everything in the index.
///
/// Behaves like `git submodule add`: the vendor files, `.gitvendors`, and
/// `.gitattributes` are written to the working tree and staged in the index,
/// but no commit is created.  The caller (or user) is expected to commit.
///
/// Returns the updated `VendorSource` wrapped in a [`MergeOutcome`].
///
/// * `name`    – unique identifier stored in `.gitvendors`
/// * `url`     – remote URL to vendor from
/// * `branch`  – upstream branch to track (`None` → HEAD)
/// * `patterns` – raw pattern strings, optionally with colon mapping syntax
///   (e.g. `["src/**:ext/"]`).  See [`PatternMapping`].
/// * `path`    – default destination prefix applied to patterns that have no
///   explicit colon mapping.  Written into `.gitvendors` as the
///   colon syntax so future merges use the same placement.
pub fn add(
    repo: &Repository,
    name: &str,
    url: &str,
    branch: Option<&str>,
    patterns: &[&str],
    path: Option<&Path>,
    file_favor: Option<git2::FileFavor>,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    if repo.get_vendor_by_name(name)?.is_some() {
        return Err(format!("vendor '{}' already exists", name).into());
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;

    // Compute the CWD relative to the workdir so that running the command
    // from a subdirectory (e.g. `cd ext/ && git vendor add ...`) behaves the
    // same as passing `--path ext/` from the repo root.
    //
    // `Repository::open_from_env` / `open` canonicalises the workdir, so we
    // do the same for CWD before stripping the prefix.
    let cwd_rel: Option<std::path::PathBuf> = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .and_then(|cwd| {
            let wd = workdir.canonicalize().ok()?;
            cwd.strip_prefix(&wd).ok().map(|p| p.to_path_buf())
        });

    // Resolve the effective destination path:
    //
    // - If `--path` was given, join it onto the CWD offset so that both
    //   `--path ext/` from the root and `cd ext/ && --path .` work correctly.
    // - If `--path` was omitted, fall back to the CWD offset so that
    //   `cd ext/ && git vendor add ...` places files under `ext/`.
    //
    // A resulting path of `""` / `"."` means "no remapping" and is treated
    // as `None`.
    let resolved_path: Option<std::path::PathBuf> = match (path, &cwd_rel) {
        // Explicit --path: join with the CWD offset so relative paths like
        // "." are anchored to where the user is standing.
        (Some(p), Some(rel)) => {
            let joined = rel.join(p);
            // Normalize away any "." components without touching the FS.
            let s = joined.to_string_lossy().replace('\\', "/");
            let s = s.trim_end_matches('/');
            if s.is_empty() || s == "." {
                None
            } else {
                Some(std::path::PathBuf::from(s))
            }
        }
        (Some(p), None) => Some(p.to_path_buf()),
        // No --path: use CWD offset (may be None when at the repo root).
        (None, rel) => rel.as_ref().and_then(|r| {
            if r == std::path::Path::new("") || r == std::path::Path::new(".") {
                None
            } else {
                Some(r.clone())
            }
        }),
    };

    let resolved_path_str: Option<String> = resolved_path.as_deref().map(|p| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string()
    });

    // Bake the resolved path into each pattern that lacks an explicit colon
    // destination, writing colon-syntax into .gitvendors (e.g. `src/**:ext/`).
    // After add, path plays no further role — patterns are self-contained.
    let resolved_patterns: Vec<String> = patterns
        .iter()
        .map(|raw| {
            let m = PatternMapping::parse(raw);
            if m.destination.is_some() {
                // Already has an explicit colon mapping — keep it.
                m.to_raw()
            } else if let Some(ref dest) = resolved_path_str {
                format!("{}:{}/", m.glob, dest)
            } else {
                m.glob
            }
        })
        .collect();

    let source = VendorSource {
        name: name.to_string(),
        url: url.to_string(),
        branch: branch.map(String::from),
        base: None,
        commit: Default::default(),
        patterns: resolved_patterns,
    };

    // Persist to .gitvendors config (create the file if it doesn't exist yet).
    {
        let mut cfg = repo
            .vendor_config()
            .or_else(|_| git2::Config::open(&workdir.join(".gitvendors")))?;
        source.to_config(&mut cfg)?;
    }

    // Fetch upstream.
    repo.fetch_vendor(&source, None)?;

    // Update base in .gitvendors to the current upstream tip.
    let vendor_ref = repo.find_reference(&source.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;
    let updated = VendorSource {
        name: source.name.clone(),
        url: source.url.clone(),
        branch: source.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
        commit: source.commit.clone(),
        patterns: source.patterns.clone(),
    };
    {
        let mut cfg = repo.vendor_config()?;
        updated.to_config(&mut cfg)?;
    }

    // Also write the base commit OID to refs/vendor/<name>/base.
    repo.reference(
        &source.base_ref(),
        vendor_commit.id(),
        true,
        "git-vendor: set initial base ref",
    )?;

    // Check for overlapping output paths with already-configured vendors.
    {
        let existing = repo.list_vendors().unwrap_or_default();
        let new_mappings = parse_patterns(&updated.patterns);

        // Collect all local paths this new vendor would produce.
        let upstream_tree = repo.find_reference(&source.head_ref())?.peel_to_tree()?;
        let mut new_paths: HashSet<String> = HashSet::new();
        upstream_tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                return git2::TreeWalkResult::Ok;
            }
            let up = format!("{}{}", dir, entry.name().unwrap_or(""));
            if let Some(local) = crate::apply_pattern_mappings(&new_mappings, &up) {
                new_paths.insert(local);
            }
            git2::TreeWalkResult::Ok
        })?;

        // Check against every other vendor's output paths.
        for other in &existing {
            if other.name == name {
                continue;
            }
            let other_mappings = parse_patterns(&other.patterns);
            if let Ok(other_ref) = repo.find_reference(&other.head_ref()) {
                if let Ok(other_tree) = other_ref.peel_to_tree() {
                    other_tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
                        if entry.kind() != Some(git2::ObjectType::Blob) {
                            return git2::TreeWalkResult::Ok;
                        }
                        let up = format!("{}{}", dir, entry.name().unwrap_or(""));
                        if let Some(local) = crate::apply_pattern_mappings(&other_mappings, &up) {
                            if new_paths.contains(&local) {
                                // Signal overlap via a sentinel path so we can
                                // detect it after the walk (walks can't early-exit
                                // with an error).
                                new_paths.insert(format!("\x00overlap:{}", local));
                            }
                        }
                        git2::TreeWalkResult::Ok
                    })?;
                    for path in &new_paths {
                        if let Some(overlap) = path.strip_prefix("\x00overlap:") {
                            return Err(format!(
                                "vendor '{}' and '{}' both map to output path '{}'; \
                                 overlapping output paths are not allowed",
                                name, other.name, overlap
                            )
                            .into());
                        }
                    }
                }
            }
        }

        // Check for collision with existing non-vendored files in HEAD.
        if let Ok(head_commit) = repo.head().and_then(|h| h.peel_to_commit()) {
            if let Ok(head_tree) = head_commit.tree() {
                for local_path in &new_paths {
                    if local_path.starts_with('\x00') {
                        continue;
                    }
                    if head_tree.get_path(Path::new(local_path)).is_ok() {
                        // File exists in HEAD — check if it belongs to another vendor.
                        let attr = repo.get_attr(
                            Path::new(local_path),
                            "vendor",
                            git2::AttrCheckFlags::FILE_THEN_INDEX,
                        );
                        match attr {
                            Ok(Some(_)) => {} // already vendored, ok
                            _ => {
                                return Err(format!(
                                    "file '{}' already exists and is not vendored; \
                                     cannot add vendor '{}' without first removing it",
                                    local_path, name
                                )
                                .into());
                            }
                        }
                    }
                }
            }
        }
    }

    // Track the stored patterns in .gitattributes.
    repo.track_vendor_pattern(&source)?;

    // Perform the initial one-time merge using the stored patterns directly,
    // since no vendor files exist in HEAD yet for `merge_vendor` to discover.
    let merged_index = repo.add_vendor(&source, file_favor)?;

    // Write merged result (including conflict markers) to the working tree
    // and stage clean entries.
    let outcome = checkout_and_stage(repo, merged_index, updated)?;

    // Stage metadata files that checkout_and_stage does not cover.
    let mut repo_index = repo.index()?;
    repo_index.add_path(Path::new(".gitvendors"))?;
    if workdir.join(".gitattributes").exists() {
        repo_index.add_path(Path::new(".gitattributes"))?;
    }
    repo_index.write()?;

    Ok(outcome)
}

/// Add glob pattern(s) to an existing vendor's configuration in `.gitvendors`.
///
/// Only edits `.gitvendors` — does not fetch, merge, or touch `.gitattributes`.
/// Run `git vendor merge` to apply the new patterns.
pub fn track_patterns(
    repo: &Repository,
    name: &str,
    patterns: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;

    for pat in patterns {
        let pat = pat.to_string();
        if !vendor.patterns.contains(&pat) {
            vendor.patterns.push(pat);
        }
    }

    let mut cfg = repo.vendor_config()?;
    vendor.to_config(&mut cfg)?;
    Ok(())
}

/// Remove glob pattern(s) from an existing vendor's configuration in `.gitvendors`.
///
/// Only edits `.gitvendors` — does not touch `.gitattributes` or the working tree.
/// Run `git vendor merge` to reconcile.
pub fn untrack_patterns(
    repo: &Repository,
    name: &str,
    patterns: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;

    let to_remove: std::collections::HashSet<&str> = patterns.iter().copied().collect();
    vendor.patterns.retain(|p| !to_remove.contains(p.as_str()));

    let mut cfg = repo.vendor_config()?;
    vendor.to_config(&mut cfg)?;
    Ok(())
}

/// Write `vendor=<name>` entries into `.gitattributes` for the given file paths.
pub fn track_attrs(
    repo: &Repository,
    name: &str,
    paths: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    // Verify the vendor exists.
    repo.get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
    let gitattributes = workdir.join(".gitattributes");
    let vendor_attr = format!("vendor={}", name);

    for path in paths {
        repo.set_attr(path, &[&vendor_attr], &gitattributes)?;
    }

    Ok(())
}

/// Remove `vendor=<name>` entries from `.gitattributes` for the given file paths.
pub fn untrack_attrs(
    repo: &Repository,
    name: &str,
    paths: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    // Verify the vendor exists.
    repo.get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
    let gitattributes = workdir.join(".gitattributes");

    if !gitattributes.exists() {
        return Ok(());
    }

    let needle = format!("vendor={}", name);
    let to_remove: std::collections::HashSet<&str> = paths.iter().copied().collect();

    let content = std::fs::read_to_string(&gitattributes)?;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            // Keep the line unless it matches this vendor attr AND its path is in `paths`.
            let mut tokens = line.split_whitespace();
            let line_path = match tokens.next() {
                Some(p) => p,
                None => return true,
            };
            let has_attr = tokens.any(|t| t == needle);
            !(has_attr && to_remove.contains(line_path))
        })
        .collect();

    if filtered.len() < content.lines().count() {
        let mut out = filtered.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        std::fs::write(&gitattributes, out)?;
    }

    Ok(())
}

/// Fetch the latest upstream commits for a single vendor.
///
/// Returns `Some(oid)` if the ref advanced, or `None` if already up-to-date.
pub fn fetch_one(
    repo: &Repository,
    name: &str,
) -> Result<Option<git2::Oid>, Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;
    let old_oid = repo
        .find_reference(&vendor.head_ref())
        .ok()
        .and_then(|r| r.target());
    let reference = repo.fetch_vendor(&vendor, None)?;
    let oid = reference
        .target()
        .ok_or_else(|| git2::Error::from_str("fetched ref is symbolic; expected a direct ref"))?;
    if old_oid == Some(oid) {
        Ok(None)
    } else {
        Ok(Some(oid))
    }
}

/// Fetch the latest upstream commits for every configured vendor.
///
/// Returns a list of `(vendor_name, oid)` pairs.
pub fn fetch_all(
    repo: &Repository,
) -> Result<Vec<(String, git2::Oid)>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let mut results = Vec::with_capacity(vendors.len());
    for v in &vendors {
        let old_oid = repo
            .find_reference(&v.head_ref())
            .ok()
            .and_then(|r| r.target());
        let reference = repo.fetch_vendor(v, None)?;
        let oid = reference.target().ok_or_else(|| {
            git2::Error::from_str("fetched ref is symbolic; expected a direct ref")
        })?;
        if old_oid != Some(oid) {
            results.push((v.name.clone(), oid));
        }
    }
    Ok(results)
}

/// Per-vendor update status returned by [`status`].
#[derive(Debug)]
pub struct VendorStatus {
    pub name: String,
    /// The state of the vendor relative to its upstream.
    pub state: VendorState,
}

/// The relationship between a vendor's local base and its upstream head.
#[derive(Debug)]
pub enum VendorState {
    /// Local base matches upstream head — nothing to merge.
    UpToDate,
    /// Upstream has new commits reachable from the current base.
    UpdateAvailable { head: git2::Oid },
    /// Upstream was force-pushed: base is no longer an ancestor of head.
    ForcePushed { head: git2::Oid },
}

/// Check every configured vendor and report its state relative to upstream.
pub fn status(repo: &Repository) -> Result<Vec<VendorStatus>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let mut out = Vec::with_capacity(vendors.len());

    for vendor in vendors {
        let head_oid = repo
            .find_reference(&vendor.head_ref())?
            .target()
            .ok_or_else(|| {
                git2::Error::from_str(&format!(
                    "refs/vendor/{}/head is a symbolic ref; expected a direct ref",
                    vendor.name
                ))
            })?;

        let state = match &vendor.base {
            None => VendorState::UpdateAvailable { head: head_oid },
            Some(base_str) => {
                let base_oid = git2::Oid::from_str(base_str)?;
                if base_oid == head_oid {
                    VendorState::UpToDate
                } else {
                    // Determine whether base is an ancestor of head.
                    let is_ancestor = repo.graph_descendant_of(head_oid, base_oid)?;
                    if is_ancestor {
                        VendorState::UpdateAvailable { head: head_oid }
                    } else {
                        VendorState::ForcePushed { head: head_oid }
                    }
                }
            }
        };

        out.push(VendorStatus {
            name: vendor.name,
            state,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Remove a vendor source: delete its `.gitvendors` entry, remove its
/// `refs/vendor/<name>/head` and `refs/vendor/<name>/base` refs, remove matching lines from `.gitattributes`
/// files, and mark vendored files as "deleted by them" conflicts in the
/// index.
///
/// The vendored files are left in the working tree.  The user resolves
/// each conflict by either accepting the deletion (`git rm <file>`) or
/// keeping the file (`git add <file>`).
pub fn rm(repo: &Repository, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;

    // Collect vendored file index entries *before* we remove gitattributes,
    // because we rely on the `vendor=<name>` attribute to identify them.
    let vendored_entries = collect_vendored_entries(repo, name)?;

    // 1. Remove the vendor section from .gitvendors.
    let vendors_path = workdir.join(".gitvendors");
    if vendors_path.exists() {
        remove_vendor_from_gitvendors(&vendors_path, name)?;
    }

    // 2. Delete refs/vendor/<name>/head and refs/vendor/<name>/base.
    if let Ok(mut reference) = repo.find_reference(&vendor.head_ref()) {
        reference.delete()?;
    }
    if let Ok(mut reference) = repo.find_reference(&vendor.base_ref()) {
        reference.delete()?;
    }

    // 3. Remove gitattributes lines that reference this vendor.
    remove_vendor_attrs(workdir, name)?;

    // 4. Stage .gitvendors (and any affected .gitattributes).
    let mut index = repo.index()?;
    index.add_path(Path::new(".gitvendors"))?;
    for entry in find_gitattributes(workdir) {
        let rel = entry.strip_prefix(workdir).unwrap_or(&entry);
        if rel.exists() || workdir.join(rel).exists() {
            index.add_path(rel)?;
        }
    }

    // 5. For each vendored file, either remove it outright (if empty) or
    //    mark it as a "deleted by them" conflict.
    for entry in &vendored_entries {
        let path = std::str::from_utf8(&entry.path)
            .map_err(|e| git2::Error::from_str(&format!("invalid UTF-8 in path: {}", e)))?;

        // Remove the clean stage-0 entry first.
        index.remove(Path::new(path), 0)?;

        if entry.file_size == 0 {
            // Empty file — just delete from working tree, no conflict needed.
            let abs = workdir.join(path);
            if abs.exists() {
                std::fs::remove_file(&abs)?;
            }
            continue;
        }

        let make_entry = |stage: u16| git2::IndexEntry {
            ctime: entry.ctime,
            mtime: entry.mtime,
            dev: entry.dev,
            ino: entry.ino,
            mode: entry.mode,
            uid: entry.uid,
            gid: entry.gid,
            file_size: entry.file_size,
            id: entry.id,
            flags: (entry.flags & 0x0FFF) | (stage << 12),
            flags_extended: entry.flags_extended,
            path: entry.path.clone(),
        };

        // Stage 1 — ancestor (base).
        index.add(&make_entry(1))?;
        // Stage 2 — ours (identical content).
        index.add(&make_entry(2))?;
        // No stage 3 → "deleted by them".
    }

    index.write()?;

    Ok(())
}

/// Collect stage-0 index entries for files attributed to the given vendor.
fn collect_vendored_entries(
    repo: &Repository,
    name: &str,
) -> Result<Vec<git2::IndexEntry>, Box<dyn std::error::Error>> {
    let index = repo.index()?;
    let mut entries = Vec::new();
    for entry in index.iter() {
        let stage = (entry.flags >> 12) & 0x3;
        if stage != 0 {
            continue;
        }
        let path = std::str::from_utf8(&entry.path)
            .map_err(|e| git2::Error::from_str(&format!("invalid UTF-8 in path: {}", e)))?;
        match repo.get_attr(
            Path::new(path),
            "vendor",
            git2::AttrCheckFlags::FILE_THEN_INDEX,
        ) {
            Ok(Some(value)) if value == name => entries.push(entry),
            _ => {}
        }
    }
    Ok(entries)
}

/// Prune `refs/vendor/*` refs that have no corresponding entry in
/// `.gitvendors`.
///
/// Returns the names of pruned refs.
pub fn prune(repo: &Repository) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let known: HashSet<String> = vendors.into_iter().map(|v| v.name).collect();

    let mut pruned = Vec::new();
    for reference in repo.references_glob("refs/vendor/*/head")? {
        let reference = reference?;
        let refname = reference.name().unwrap_or("").to_string();
        // refname is "refs/vendor/<name>/head"
        let vendor_name = refname
            .strip_prefix("refs/vendor/")
            .and_then(|s| s.strip_suffix("/head"))
            .unwrap_or("");
        if !vendor_name.is_empty() && !known.contains(vendor_name) {
            pruned.push(vendor_name.to_string());
        }
    }

    for name in &pruned {
        let head_ref = format!("refs/vendor/{}/head", name);
        if let Ok(mut r) = repo.find_reference(&head_ref) {
            r.delete()?;
        }
        let base_ref = format!("refs/vendor/{}/base", name);
        if let Ok(mut r) = repo.find_reference(&base_ref) {
            r.delete()?;
        }
    }

    Ok(pruned)
}

// ---------------------------------------------------------------------------
// Helpers for `rm`
// ---------------------------------------------------------------------------

/// Remove the `[vendor "<name>"]` section (and its keys) from a
/// `.gitvendors` file, rewriting it in place.
fn remove_vendor_from_gitvendors(
    path: &Path,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let header = format!("[vendor \"{}\"]", name);
    let mut out = String::new();
    let mut skip = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == header {
            skip = true;
            continue;
        }
        // A new section header ends the skip region.
        if skip && trimmed.starts_with('[') {
            skip = false;
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }

    std::fs::write(path, out)?;
    Ok(())
}

/// Walk the working tree for `.gitattributes` files, returning their absolute
/// paths.
fn find_gitattributes(workdir: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    fn walk(dir: &Path, results: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip .git directory.
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                walk(&path, results);
            } else if path.file_name().is_some_and(|n| n == ".gitattributes") {
                results.push(path);
            }
        }
    }
    walk(workdir, &mut results);
    results
}

/// Remove lines containing `vendor=<name>` from all `.gitattributes` files
/// under the working tree.
fn remove_vendor_attrs(workdir: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let needle = format!("vendor={}", name);
    for attr_path in find_gitattributes(workdir) {
        let content = std::fs::read_to_string(&attr_path)?;
        let filtered: Vec<&str> = content
            .lines()
            .filter(|line| !line.split_whitespace().any(|token| token == needle))
            .collect();
        // Only rewrite if something changed.
        if filtered.len() < content.lines().count() {
            let mut out = filtered.join("\n");
            if !out.is_empty() {
                out.push('\n');
            }
            std::fs::write(&attr_path, out)?;
        }
    }
    Ok(())
}

/// Result of a single vendor merge.
pub enum MergeOutcome {
    /// The vendor's `base` already matches the latest `refs/vendor/$name/head`.
    /// Nothing was changed.
    UpToDate {
        /// The vendor source (unchanged).
        vendor: VendorSource,
    },
    /// The merge completed cleanly.  All changes are staged in the index and
    /// written to the working tree, but no commit has been created.
    Clean {
        /// The vendor source with updated `base`.
        vendor: VendorSource,
    },
    /// The merge has conflicts.  Conflict markers have been written to the
    /// working tree.  The returned `git2::Index` contains the conflict
    /// entries.  The caller is responsible for presenting them to the user.
    /// `base` has still been updated in `.gitvendors`.
    Conflict {
        index: git2::Index,
        /// The vendor source with updated `base`.
        vendor: VendorSource,
    },
}

/// Merge upstream changes for a single vendor.
///
/// Writes the merged result to the working tree and stages it in the index.
/// Always updates the vendor's `base` in `.gitvendors`.  No commit is created.
///
/// Returns the updated `VendorSource` wrapped in a [`MergeOutcome`].
pub fn merge_one(
    repo: &Repository,
    name: &str,
    file_favor: Option<git2::FileFavor>,
    no_commit: bool,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;
    merge_vendor(repo, &vendor, file_favor, no_commit)
}

/// Merge upstream changes for every configured vendor.
///
/// Returns one `(vendor_name, MergeOutcome)` per vendor, in the order they
/// were processed.  Processing stops at the first error.
pub fn merge_all(
    repo: &Repository,
    file_favor: Option<git2::FileFavor>,
    no_commit: bool,
) -> Result<Vec<(String, MergeOutcome)>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let mut results = Vec::with_capacity(vendors.len());
    for v in &vendors {
        let outcome = merge_vendor(repo, v, file_favor, no_commit)?;
        results.push((v.name.clone(), outcome));
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write the contents of a merged index to the working tree using libgit2's
/// checkout machinery, then stage any cleanly-resolved entries in the
/// repository's own index.
///
/// The checkout is scoped to only the paths present in `merged_index` so that
/// unrelated working-tree files are left untouched.
///
/// Conflict markers are written for conflicted files using the merge style
/// (`<<<<<<< ours` / `=======` / `>>>>>>> theirs`).
///
/// Returns `MergeOutcome::Conflict` when the index has conflicts, or
/// `MergeOutcome::Clean` otherwise.
fn checkout_and_stage(
    repo: &Repository,
    mut merged_index: git2::Index,
    vendor: VendorSource,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let has_conflicts = merged_index.has_conflicts();

    // Collect every path in the merged index so we can scope the checkout.
    let paths: Vec<String> = merged_index
        .iter()
        .filter_map(|entry| std::str::from_utf8(&entry.path).ok().map(String::from))
        .collect();

    // Write merged entries (and conflict markers) to the working directory,
    // touching only the paths that are part of this merge.
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    checkout.allow_conflicts(true);
    checkout.conflict_style_merge(true);
    for p in &paths {
        checkout.path(p);
    }
    repo.checkout_index(Some(&mut merged_index), Some(&mut checkout))?;

    // Stage cleanly-resolved (stage 0) entries in the repository index.
    let mut repo_index = repo.index()?;
    for entry in merged_index.iter() {
        let stage = (entry.flags >> 12) & 0x3;
        if stage != 0 {
            continue;
        }
        let entry_path = std::str::from_utf8(&entry.path)
            .map_err(|e| git2::Error::from_str(&format!("invalid UTF-8 in path: {}", e)))?;
        repo_index.add_path(Path::new(entry_path))?;
    }
    repo_index.write()?;

    if has_conflicts {
        Ok(MergeOutcome::Conflict {
            index: merged_index,
            vendor,
        })
    } else {
        Ok(MergeOutcome::Clean { vendor })
    }
}

/// Merge a single vendor's upstream into the working tree and stage the
/// result.  Always updates `base` in `.gitvendors` to the current upstream
/// tip.  No commit is created.
///
/// Returns the updated `VendorSource` wrapped in a [`MergeOutcome`].
fn merge_vendor(
    repo: &Repository,
    vendor: &VendorSource,
    file_favor: Option<git2::FileFavor>,
    no_commit: bool,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    if no_commit && vendor.commit == CommitMode::Replay {
        return Err("--no-commit is incompatible with the `replay` commit mode".into());
    }
    let vendor_ref = repo.find_reference(&vendor.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;

    // Nothing to do when the base already matches the upstream tip.
    if let Some(base) = &vendor.base
        && git2::Oid::from_str(base)? == vendor_commit.id()
    {
        return Ok(MergeOutcome::UpToDate {
            vendor: vendor.clone(),
        });
    }

    // Capture the old base OID before updating so we can walk base..head.
    let old_base_oid = vendor
        .base
        .as_deref()
        .and_then(|b| git2::Oid::from_str(b).ok());

    // Build the updated VendorSource with the new base, but do NOT write it
    // to .gitvendors yet — that happens only after a successful commit or
    // staging (invariant: base is written only on success).
    let updated = VendorSource {
        name: vendor.name.clone(),
        url: vendor.url.clone(),
        branch: vendor.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
        commit: vendor.commit.clone(),
        patterns: vendor.patterns.clone(),
    };

    let mut merged_index = repo.merge_vendor(vendor, None, file_favor)?;

    // Refresh .gitattributes: add entries for new upstream files, remove
    // entries for files deleted upstream.  Use the pattern-filtered upstream
    // tree as the authoritative source of which files belong to this vendor.
    let upstream_tree = repo.find_reference(&vendor.head_ref())?.peel_to_tree()?;
    let mappings = parse_patterns(&vendor.patterns);
    let theirs_remapped = remap_upstream_tree(&repo, &upstream_tree, &mappings)?;
    repo.refresh_vendor_attrs(vendor, &theirs_remapped, Path::new("."))?;

    // Remove any entry from the merged index whose path is not already
    // attributed to this vendor in HEAD.  New upstream files that match
    // patterns but have never been attributed must not be introduced into
    // the working tree automatically.
    let expected_vendor = vendor.name.clone();
    let paths_to_remove: Vec<String> = merged_index
        .iter()
        .filter_map(|entry| std::str::from_utf8(&entry.path).ok().map(String::from))
        .filter(|path| {
            match repo.get_attr(Path::new(path), "vendor", git2::AttrCheckFlags::INDEX_ONLY) {
                Ok(Some(value)) => value != expected_vendor,
                _ => true,
            }
        })
        .collect();
    for path in &paths_to_remove {
        let _ = merged_index.remove_path(Path::new(path));
    }

    let outcome = checkout_and_stage(repo, merged_index, updated)?;

    // Write base to .gitvendors and advance refs/vendor/<name>/base now that
    // the working tree and index are in a consistent state.  This covers both
    // clean merges (committed or staged) and conflicts (staged for resolution).
    {
        let mut cfg = repo.vendor_config()?;
        let updated_vendor = match &outcome {
            MergeOutcome::Clean { vendor } | MergeOutcome::Conflict { vendor, .. } => vendor,
            MergeOutcome::UpToDate { .. } => unreachable!(),
        };
        updated_vendor.to_config(&mut cfg)?;
    }
    repo.reference(
        &vendor.base_ref(),
        vendor_commit.id(),
        true,
        "git-vendor: update base ref",
    )?;

    // Stage metadata files that checkout_and_stage does not cover.
    let mut repo_index = repo.index()?;
    repo_index.add_path(Path::new(".gitvendors"))?;
    if Path::new(".gitattributes").exists() {
        repo_index.add_path(Path::new(".gitattributes"))?;
    }
    repo_index.write()?;

    match &outcome {
        MergeOutcome::Clean {
            vendor: updated_vendor,
        } => {
            if no_commit {
                write_vendor_msg(repo, updated_vendor, old_base_oid, &vendor_commit, false)?;
            } else {
                commit_vendor_merge(repo, updated_vendor, old_base_oid, &vendor_commit)?;
            }
        }
        MergeOutcome::Conflict {
            vendor: updated_vendor,
            ..
        } => {
            write_vendor_msg(repo, updated_vendor, old_base_oid, &vendor_commit, true)?;
        }
        MergeOutcome::UpToDate { .. } => unreachable!("up-to-date vendor after staging"),
    }

    Ok(outcome)
}

/// Collect upstream commits in the range `old_base..head` (oldest-first).
///
/// When `old_base` is `None` (first-time add), returns only `head`.
fn collect_upstream_commits<'a>(
    repo: &'a Repository,
    old_base: Option<git2::Oid>,
    head: &git2::Commit<'a>,
) -> Result<Vec<git2::Commit<'a>>, git2::Error> {
    let mut walk = repo.revwalk()?;
    walk.push(head.id())?;
    if let Some(base_oid) = old_base {
        walk.hide(base_oid)?;
    }
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
    let mut commits = Vec::new();
    for oid in walk {
        commits.push(repo.find_commit(oid?)?);
    }
    Ok(commits)
}

/// Summarize author contributions across a list of commits.
///
/// Returns a `Vec` of `(name, email, count)` sorted by descending count.
fn author_summary(commits: &[git2::Commit<'_>]) -> Vec<(String, String, usize)> {
    let mut map: HashMap<(String, String), usize> = HashMap::new();
    for c in commits {
        let sig = c.author();
        let name = sig.name().unwrap_or("Unknown").to_string();
        let email = sig.email().unwrap_or("").to_string();
        *map.entry((name, email)).or_insert(0) += 1;
    }
    let mut v: Vec<_> = map
        .into_iter()
        .map(|((n, e), count)| (n, e, count))
        .collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    v
}

/// Diff stats between two trees (or an empty tree vs `new_tree` when `old_tree` is None).
struct DiffStats {
    added: usize,
    removed: usize,
    modified: usize,
}

fn diff_stats(
    repo: &Repository,
    old_tree: Option<&git2::Tree<'_>>,
    new_tree: &git2::Tree<'_>,
) -> Result<DiffStats, git2::Error> {
    let diff = repo.diff_tree_to_tree(old_tree, Some(new_tree), None)?;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut modified = 0usize;
    for delta in diff.deltas() {
        match delta.status() {
            git2::Delta::Added => added += 1,
            git2::Delta::Deleted => removed += 1,
            _ => modified += 1,
        }
    }
    Ok(DiffStats {
        added,
        removed,
        modified,
    })
}

/// Build the VENDOR_MSG body (subject + body paragraphs, no trailing newline).
pub(crate) fn build_vendor_msg(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let old_base_short = old_base_oid
        .map(|o| o.to_string()[..7].to_string())
        .unwrap_or_else(|| "0000000".to_string());
    let head_short = &head_commit.id().to_string()[..7];
    let branch = vendor.branch.as_deref().unwrap_or("HEAD");

    let commits = collect_upstream_commits(repo, old_base_oid, head_commit)?;
    let authors = author_summary(&commits);

    // Diff stats: compare old base tree vs new head tree.
    let old_tree = old_base_oid
        .and_then(|o| repo.find_commit(o).ok())
        .and_then(|c| c.tree().ok());
    let new_tree = head_commit.tree()?;
    let stats = diff_stats(repo, old_tree.as_ref(), &new_tree)?;
    let total = stats.added + stats.removed + stats.modified;

    let subject = format!(
        "Vendor update: {} {} ({}...{})",
        vendor.name, branch, old_base_short, head_short,
    );

    let body = format!(
        "Updated {} files. {} added, {} removed, {} modified.",
        total, stats.added, stats.removed, stats.modified,
    );

    let author_trailers: Vec<String> = authors
        .iter()
        .map(|(name, email, count)| {
            format!("Upstream-Author: {} <{}> ({} commits)", name, email, count)
        })
        .collect();

    let mut parts = vec![subject, String::new(), body, String::new()];
    parts.extend(author_trailers);
    Ok(parts.join("\n"))
}

/// Write a prepared commit message to `.git/VENDOR_MSG`.
///
/// When `is_conflict` is true, appends instructions for the user to resolve
/// conflicts and commit manually.
fn write_vendor_msg(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
    is_conflict: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut msg = build_vendor_msg(repo, vendor, old_base_oid, head_commit)?;

    if is_conflict {
        msg.push_str(
            "\n\n# Conflicts detected. Resolve them, then commit with:\n\
             #   git commit -e -F .git/VENDOR_MSG",
        );
    }

    msg.push('\n');
    let git_dir = repo.path();
    std::fs::write(git_dir.join("VENDOR_MSG"), &msg)?;
    Ok(())
}

/// Create the local commit(s) after a clean merge, according to `vendor.commit`.
fn commit_vendor_merge(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    match &vendor.commit {
        CommitMode::Squash => commit_squash(repo, vendor, old_base_oid, head_commit),
        CommitMode::Linear => commit_linear(repo, vendor, old_base_oid, head_commit),
        CommitMode::Replay => commit_replay(repo, vendor, old_base_oid, head_commit),
    }
}

/// Local git config signature (committer / author for local commits).
fn local_signature(repo: &Repository) -> Result<git2::Signature<'static>, git2::Error> {
    let cfg = repo.config()?;
    let name = cfg
        .get_string("user.name")
        .unwrap_or_else(|_| "git-vendor".to_string());
    let email = cfg
        .get_string("user.email")
        .unwrap_or_else(|_| "git-vendor@localhost".to_string());
    git2::Signature::now(&name, &email)
}

/// Squash mode: create a synthetic second-parent commit whose tree is the
/// remapped upstream tree, then a merge commit with HEAD + that synthetic
/// commit as parents.
fn commit_squash(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sig = local_signature(repo)?;
    let msg = build_vendor_msg(repo, vendor, old_base_oid, head_commit)?;

    // The synthetic squash commit has the upstream tree (not remapped — it
    // represents the upstream state for DAG purposes) and no parents.
    let upstream_tree = head_commit.tree()?;
    let squash_oid = repo.commit(None, &sig, &sig, &msg, &upstream_tree, &[])?;
    let squash_commit = repo.find_commit(squash_oid)?;

    // HEAD commit.
    let head_local = repo.head()?.peel_to_commit()?;

    // The merge commit tree comes from the staged index.
    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let merge_tree = repo.find_tree(tree_oid)?;

    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &format!("{}\n", msg),
        &merge_tree,
        &[&head_local, &squash_commit],
    )?;

    // Clear the merge state so git doesn't think there's a pending merge.
    repo.cleanup_state()?;
    Ok(())
}

/// Linear mode: single-parent commit on HEAD.
fn commit_linear(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sig = local_signature(repo)?;
    let msg = build_vendor_msg(repo, vendor, old_base_oid, head_commit)?;

    let head_local = repo.head()?.peel_to_commit()?;
    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &format!("{}\n", msg),
        &tree,
        &[&head_local],
    )?;

    repo.cleanup_state()?;
    Ok(())
}

/// Replay mode: walk base..head commits and replay each one onto HEAD,
/// preserving original author identity.
fn commit_replay(
    repo: &Repository,
    vendor: &VendorSource,
    old_base_oid: Option<git2::Oid>,
    head_commit: &git2::Commit<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let committer = local_signature(repo)?;
    let mappings = parse_patterns(&vendor.patterns);
    let commits = collect_upstream_commits(repo, old_base_oid, head_commit)?;

    if commits.is_empty() {
        return Ok(());
    }

    let mut parent_oid = repo.head()?.peel_to_commit()?.id();

    for upstream_commit in &commits {
        let upstream_tree = upstream_commit.tree()?;
        let remapped_tree = remap_upstream_tree(repo, &upstream_tree, &mappings)?;

        let author = upstream_commit.author();
        // Preserve original author time; committer is local user with now.
        let author_sig = git2::Signature::new(
            author.name().unwrap_or("Unknown"),
            author.email().unwrap_or(""),
            &author.when(),
        )?;

        let parent_commit = repo.find_commit(parent_oid)?;
        let msg = upstream_commit.message().unwrap_or("").to_string();

        let new_oid = repo.commit(
            Some("HEAD"),
            &author_sig,
            &committer,
            &msg,
            &remapped_tree,
            &[&parent_commit],
        )?;
        parent_oid = new_oid;
    }

    repo.cleanup_state()?;
    Ok(())
}
