use crate::Error;

/// A vendored dependency entry loaded from configuration.
pub struct VendorEntry {
    pub name: String,
    pub url: String,
    pub base: Option<gix::ObjectId>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
}

pub trait DepotRepository {
    /// Return all vendor entries from `.gitvendors`.
    fn list_vendors(&self) -> Result<Vec<VendorEntry>, Error>;

    /// Look up a vendor entry by name.
    fn get_vendor(&self, name: &str) -> Result<Option<VendorEntry>, Error>;

    /// Perform the initial placement of a vendor's files into the working tree.
    fn add_vendor(&self, entry: &VendorEntry) -> Result<gix::index::State, Error>;

    /// Remove a vendor entry from `.gitvendors`.
    fn remove_vendor(&self, name: &str) -> Result<(), Error>;

    /// Fetch remote content into `refs/vendor/$name`, returning the tip object ID.
    fn fetch_vendor(&self, entry: &VendorEntry) -> Result<gix::ObjectId, Error>;

    /// Write `.gitattributes` entries for all files matched by a vendor's patterns.
    fn track_vendor_pattern(&self, entry: &VendorEntry) -> Result<(), Error>;

    /// Refresh `.gitattributes` after a merge to reflect the current vendored file set.
    fn refresh_vendor_attrs(&self, entry: &VendorEntry) -> Result<(), Error>;

    /// Merge vendored content into the local tree, returning the merged index state.
    fn merge_vendor(&self, entry: &VendorEntry) -> Result<gix::index::State, Error>;

    /// Return the paths attributed to a vendor via `.gitattributes`.
    fn vendor_attributes(&self, entry: &VendorEntry) -> Result<Vec<String>, Error>;
}
