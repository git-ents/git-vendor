#![doc = include_str!("../README.md")]

pub mod cli;
pub mod exe;

use std::{collections::HashMap, path::PathBuf, str::FromStr};

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

    /// Fetch the upstream for the given vendor and advance `refs/vendor/$name`.
    /// Returns the updated reference.
    fn fetch_vendor<'a>(
        &'a self,
        source: &VendorSource,
        maybe_opts: Option<&mut git2::FetchOptions>,
    ) -> Result<git2::Reference<'a>, git2::Error>;

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
    ) -> Result<(git2::Commit<'_>, VendorSource), git2::Error>;

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
        let cfg = git2::Config::open(&self.path().join(".gitvendors"))?;
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

    fn merge_vendor(
        &self,
        vendor: &VendorSource,
        _maybe_opts: Option<&mut git2::FetchOptions>,
    ) -> Result<(git2::Commit<'_>, VendorSource), git2::Error> {
        let _theirs = self.find_reference(&vendor.head_ref())?.peel_to_tree()?;
        let _ours = self.head()?.peel_to_tree()?;

        todo!()
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
