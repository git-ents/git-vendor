use std::collections::HashSet;
use std::path::Path;

use git2::Repository;

use crate::Vendor;
use crate::VendorSource;

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
/// * `patterns` – glob(s) selecting which upstream files to vendor (e.g. `["**"]`)
/// * `path`     – local directory for vendored files; defaults to `"."`
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

    let source = VendorSource {
        name: name.to_string(),
        url: url.to_string(),
        branch: branch.map(String::from),
        base: None,
        patterns: patterns.iter().map(|s| s.to_string()).collect(),
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

    // Track the requested pattern(s).
    let path = path.unwrap_or_else(|| Path::new("."));
    repo.track_vendor_pattern(&source, patterns, path)?;

    // Update base in .gitvendors to the current upstream tip.
    let vendor_ref = repo.find_reference(&source.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;
    let updated = VendorSource {
        name: source.name.clone(),
        url: source.url.clone(),
        branch: source.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
        patterns: source.patterns.clone(),
    };
    {
        let mut cfg = repo.vendor_config()?;
        updated.to_config(&mut cfg)?;
    }

    // Perform the initial one-time merge using the glob pattern(s) directly,
    // since no vendor files exist in HEAD yet for `merge_vendor` to discover.
    let merged_index = repo.add_vendor(&source, patterns, path, file_favor)?;

    // Write merged result (including conflict markers) to the working tree
    // and stage clean entries.
    let outcome = checkout_and_stage(repo, merged_index, updated)?;

    // Stage metadata files that checkout_and_stage does not cover.
    let mut repo_index = repo.index()?;
    repo_index.add_path(Path::new(".gitvendors"))?;
    let attr_path = if path == Path::new(".") {
        std::path::PathBuf::from(".gitattributes")
    } else {
        path.join(".gitattributes")
    };
    if workdir.join(&attr_path).exists() {
        repo_index.add_path(&attr_path)?;
    }
    repo_index.write()?;

    Ok(outcome)
}

/// Add pattern(s) to an existing vendor's configuration in `.gitvendors`.
///
/// Only edits `.gitvendors` — does not fetch, merge, or touch `.gitattributes`.
/// Run `git vendor merge` to apply the new patterns.
pub fn track(
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

/// Remove pattern(s) from an existing vendor's configuration in `.gitvendors`.
///
/// Only edits `.gitvendors` — does not touch `.gitattributes` or the working tree.
/// Run `git vendor merge` to reconcile.
pub fn untrack(
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
    /// `Some(oid)` when upstream has unmerged changes at that commit;
    /// `None` when the vendor is up to date.
    pub upstream_oid: Option<git2::Oid>,
}

/// Check every configured vendor and report whether it has unmerged upstream
/// changes.
pub fn status(repo: &Repository) -> Result<Vec<VendorStatus>, Box<dyn std::error::Error>> {
    let statuses = repo.check_vendors()?;
    let mut out: Vec<VendorStatus> = statuses
        .into_iter()
        .map(|(vendor, maybe_oid)| VendorStatus {
            name: vendor.name,
            upstream_oid: maybe_oid,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Remove a vendor source: delete its `.gitvendors` entry, remove its
/// `refs/vendor/<name>` ref, remove matching lines from `.gitattributes`
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

    // 2. Delete refs/vendor/<name>.
    if let Ok(mut reference) = repo.find_reference(&vendor.head_ref()) {
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
    for reference in repo.references_glob("refs/vendor/*")? {
        let reference = reference?;
        let refname = reference.name().unwrap_or("").to_string();
        let vendor_name = refname.strip_prefix("refs/vendor/").unwrap_or("");
        if !vendor_name.is_empty() && !known.contains(vendor_name) {
            pruned.push(vendor_name.to_string());
        }
    }

    for name in &pruned {
        let refname = format!("refs/vendor/{}", name);
        if let Ok(mut r) = repo.find_reference(&refname) {
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
                if path.file_name().map_or(false, |n| n == ".git") {
                    continue;
                }
                walk(&path, results);
            } else if path.file_name().map_or(false, |n| n == ".gitattributes") {
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
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;
    merge_vendor(repo, &vendor, file_favor)
}

/// Merge upstream changes for every configured vendor.
///
/// Returns one `(vendor_name, MergeOutcome)` per vendor, in the order they
/// were processed.  Processing stops at the first error.
pub fn merge_all(
    repo: &Repository,
    file_favor: Option<git2::FileFavor>,
) -> Result<Vec<(String, MergeOutcome)>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let mut results = Vec::with_capacity(vendors.len());
    for v in &vendors {
        let outcome = merge_vendor(repo, v, file_favor)?;
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
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let vendor_ref = repo.find_reference(&vendor.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;

    // Always update base in .gitvendors to the current upstream tip.
    let updated = VendorSource {
        name: vendor.name.clone(),
        url: vendor.url.clone(),
        branch: vendor.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
        patterns: vendor.patterns.clone(),
    };
    {
        let mut cfg = repo.vendor_config()?;
        updated.to_config(&mut cfg)?;
    }

    let merged_index = repo.merge_vendor(vendor, None, file_favor)?;

    checkout_and_stage(repo, merged_index, updated)
}
