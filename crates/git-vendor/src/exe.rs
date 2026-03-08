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

/// Register a new vendor source, fetch its upstream, and set up attribute
/// tracking for the given file pattern.
///
/// * `name`       – unique identifier stored in `.gitvendors`
/// * `url`        – remote URL to vendor from
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
) -> Result<(), Box<dyn std::error::Error>> {
    if repo.get_vendor_by_name(name)?.is_some() {
        return Err(format!("vendor '{}' already exists", name).into());
    }

    let source = VendorSource {
        name: name.to_string(),
        url: url.to_string(),
        branch: branch.map(String::from),
        base: None,
    };

    // Persist to .gitvendors config (create the file if it doesn't exist yet).
    let mut cfg = repo.vendor_config().or_else(|_| {
        let workdir = repo
            .workdir()
            .ok_or_else(|| git2::Error::from_str("repository has no working directory"))?;
        git2::Config::open(&workdir.join(".gitvendors"))
    })?;
    source.to_config(&mut cfg)?;

    // Fetch upstream
    repo.fetch_vendor(&source, None)?;

    // Track the requested pattern
    let path = path.unwrap_or_else(|| Path::new("."));
    repo.track_vendor_pattern(&source, pattern, path)?;

    Ok(())
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
    /// The merge completed without conflicts and a merge commit was created.
    Clean {
        /// The OID of the new merge commit on HEAD.
        merge_commit: git2::Oid,
    },
    /// The merge has conflicts. The caller is responsible for presenting them
    /// to the user. The returned `git2::Index` contains the conflict entries.
    Conflict { index: git2::Index },
}

/// Merge upstream changes for a single vendor.
///
/// On a clean merge this creates a merge commit on `HEAD` and updates the
/// vendor's `base` in `.gitvendors` so that future merges are three-way.
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

fn merge_vendor(
    repo: &Repository,
    vendor: &VendorSource,
) -> Result<MergeOutcome, Box<dyn std::error::Error>> {
    let mut index = repo.merge_vendor(vendor, None)?;

    if index.has_conflicts() {
        return Ok(MergeOutcome::Conflict { index });
    }

    // Write the merged tree and create a merge commit.
    let tree_oid = index.write_tree_to(repo)?;
    let tree = repo.find_tree(tree_oid)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let vendor_ref = repo.find_reference(&vendor.head_ref())?;
    let vendor_commit = vendor_ref.peel_to_commit()?;
    let sig = repo.signature()?;

    let message = format!("Merge vendor '{}' at {}", vendor.name, vendor_commit.id());
    let merge_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &message,
        &tree,
        &[&head_commit, &vendor_commit],
    )?;

    // Update the base in .gitvendors so the next merge is three-way.
    let updated = VendorSource {
        name: vendor.name.clone(),
        url: vendor.url.clone(),
        branch: vendor.branch.clone(),
        base: Some(vendor_commit.id().to_string()),
    };
    let mut cfg = repo.vendor_config()?;
    updated.to_config(&mut cfg)?;

    Ok(MergeOutcome::Clean { merge_commit })
}
