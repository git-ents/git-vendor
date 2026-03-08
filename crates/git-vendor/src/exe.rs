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
/// * `pattern` – glob selecting which upstream files to vendor (e.g. `"**"`)
/// * `path`    – local directory for vendored files; defaults to `"."`
pub fn add(
    repo: &Repository,
    name: &str,
    url: &str,
    branch: Option<&str>,
    pattern: &str,
    path: Option<&Path>,
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

    // Perform the initial one-time merge using the glob pattern directly,
    // since no vendor files exist in HEAD yet for `merge_vendor` to discover.
    let merged_index = repo.add_vendor(&source, pattern, path)?;

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
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
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

    checkout_and_stage(repo, merged_index, updated)
}
