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
/// Returns the `VendorSource` with its `base` set to the fetched upstream tip.
///
/// * `name`    – unique identifier stored in `.gitvendors`
/// * `url`     – remote URL to vendor from
/// * `branch`  – upstream branch to track (`None` → HEAD)
/// * `pattern` – glob selecting which upstream files to vendor (e.g. `"**"`)
/// * `path`    – local directory for vendored files; defaults to `"."`
pub fn add(
    repo: &Repository,
    name: &str,
    url: &str,
    branch: Option<&str>,
    pattern: &str,
    path: Option<&Path>,
) -> Result<VendorSource, Box<dyn std::error::Error>> {
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

    // Track the requested pattern.
    let path = path.unwrap_or_else(|| Path::new("."));
    repo.track_vendor_pattern(&source, pattern, path)?;

    // Perform the initial one-time merge using the glob pattern directly,
    // since no vendor files exist in HEAD yet for `merge_vendor` to discover.
    let merged_index = repo.add_vendor(&source, pattern, path)?;

    // Update base in .gitvendors to the current upstream tip.
    let vendor_ref = repo.find_reference(&source.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;
    let updated = VendorSource {
        name: source.name.clone(),
        url: source.url.clone(),
        branch: source.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
    };
    {
        let mut cfg = repo.vendor_config()?;
        updated.to_config(&mut cfg)?;
    }

    // Write each merged entry to the working directory and stage it.
    let mut repo_index = repo.index()?;
    for entry in merged_index.iter() {
        let blob = repo.find_blob(entry.id)?;
        let entry_path = std::str::from_utf8(&entry.path)
            .map_err(|e| git2::Error::from_str(&format!("invalid UTF-8 in path: {}", e)))?;
        let full_path = workdir.join(entry_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, blob.content())?;
        repo_index.add_path(Path::new(entry_path))?;
    }

    // Stage metadata files.
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

    Ok(updated)
}

/// Fetch the latest upstream commits for a single vendor.
///
/// Returns the OID that `refs/vendor/<name>` now points to.
pub fn fetch_one(repo: &Repository, name: &str) -> Result<git2::Oid, Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;
    let reference = repo.fetch_vendor(&vendor, None)?;
    let oid = reference
        .target()
        .ok_or_else(|| git2::Error::from_str("fetched ref is symbolic; expected a direct ref"))?;
    Ok(oid)
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
        let reference = repo.fetch_vendor(v, None)?;
        let oid = reference.target().ok_or_else(|| {
            git2::Error::from_str("fetched ref is symbolic; expected a direct ref")
        })?;
        results.push((v.name.clone(), oid));
    }
    Ok(results)
}

/// Per-vendor update status returned by [`check`].
#[derive(Debug)]
pub struct VendorStatus {
    pub name: String,
    /// `Some(oid)` when upstream has unmerged changes at that commit;
    /// `None` when the vendor is up to date.
    pub upstream_oid: Option<git2::Oid>,
}

/// Check every configured vendor and report whether it has unmerged upstream
/// changes.
pub fn check(repo: &Repository) -> Result<Vec<VendorStatus>, Box<dyn std::error::Error>> {
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

/// Result of a single vendor merge.
pub enum MergeOutcome {
    /// The merge completed cleanly.  All changes are staged in the index and
    /// written to the working tree, but no commit has been created.
    Clean {
        /// The vendor source with updated `base`.
        vendor: VendorSource,
    },
    /// The merge has conflicts.  The returned `git2::Index` contains the
    /// conflict entries.  The caller is responsible for presenting them to
    /// the user.  `base` has still been updated in `.gitvendors`.
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
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let vendor = repo
        .get_vendor_by_name(name)?
        .ok_or_else(|| format!("vendor '{}' not found", name))?;
    merge_vendor(repo, &vendor)
}

/// Merge upstream changes for every configured vendor.
///
/// Returns one `(vendor_name, MergeOutcome)` per vendor, in the order they
/// were processed.  Processing stops at the first error.
pub fn merge_all(
    repo: &Repository,
) -> Result<Vec<(String, MergeOutcome)>, Box<dyn std::error::Error>> {
    let vendors = repo.list_vendors()?;
    let mut results = Vec::with_capacity(vendors.len());
    for v in &vendors {
        let outcome = merge_vendor(repo, v)?;
        results.push((v.name.clone(), outcome));
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Merge a single vendor's upstream into the working tree and stage the
/// result.  Always updates `base` in `.gitvendors` to the current upstream
/// tip.  No commit is created.
///
/// Returns the updated `VendorSource`.
fn merge_vendor(
    repo: &Repository,
    vendor: &VendorSource,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;

    let vendor_ref = repo.find_reference(&vendor.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;

    // Always update base in .gitvendors to the current upstream tip.
    let updated = VendorSource {
        name: vendor.name.clone(),
        url: vendor.url.clone(),
        branch: vendor.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
    };
    {
        let mut cfg = repo.vendor_config()?;
        updated.to_config(&mut cfg)?;
    }

    let merged_index = repo.merge_vendor(vendor, None)?;

    if merged_index.has_conflicts() {
        // Even with conflicts we have already persisted `base`.
        return Ok(MergeOutcome::Conflict {
            index: merged_index,
            vendor: updated,
        });
    }

    // Write each merged entry to the working directory and stage it.
    let mut repo_index = repo.index()?;
    for entry in merged_index.iter() {
        let blob = repo.find_blob(entry.id)?;
        let entry_path = std::str::from_utf8(&entry.path)
            .map_err(|e| git2::Error::from_str(&format!("invalid UTF-8 in path: {}", e)))?;
        let full_path = workdir.join(entry_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, blob.content())?;
        repo_index.add_path(Path::new(entry_path))?;
    }
    repo_index.write()?;

    Ok(MergeOutcome::Clean { vendor: updated })
}
