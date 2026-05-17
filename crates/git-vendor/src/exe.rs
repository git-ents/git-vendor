//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use crate::{Error, VendorEntry};

/// Projection of vendor state onto the working copy.
///
/// Separated from [`VendorRepository`](crate::VendorRepository) because every
/// method here writes files in the working tree (including tracked files like
/// `.gitattributes`), which is a distinct concern from the pure object-database
/// operations.
pub trait VendorWorktree {
    /// Update the index and working tree for the vendor's paths to match `tree`.
    ///
    /// Only paths owned by this vendor are written; unrelated files are left
    /// untouched.
    fn checkout_vendor(&self, entry: &VendorEntry, tree: gix::ObjectId) -> Result<(), Error>;

    /// Add the given paths to the vendor's local content filter by writing
    /// `vendor=<name>` entries into the working-copy `.gitattributes`.
    ///
    /// This authors local-side membership (read back by
    /// [`VendorRepository::vendor_paths`](crate::VendorRepository::vendor_paths));
    /// it is independent of the upstream pattern filter.
    fn track_vendor(&self, entry: &VendorEntry, paths: &[&str]) -> Result<(), Error>;

    /// Remove the given paths from the vendor's content filter, deleting their
    /// `vendor=<name>` entries from the working-copy `.gitattributes`.
    fn untrack_vendor(&self, entry: &VendorEntry, paths: &[&str]) -> Result<(), Error>;

    /// Rewrite the working-copy `.gitattributes` so its `vendor=<name>` entries
    /// match the vendor's file set after a merge.
    fn sync_attributes(&self, entry: &VendorEntry) -> Result<(), Error>;
}

impl VendorWorktree for gix::Repository {
    fn checkout_vendor(&self, _entry: &VendorEntry, _tree: gix::ObjectId) -> Result<(), Error> {
        todo!()
    }

    fn track_vendor(&self, _entry: &VendorEntry, _paths: &[&str]) -> Result<(), Error> {
        todo!()
    }

    fn untrack_vendor(&self, _entry: &VendorEntry, _paths: &[&str]) -> Result<(), Error> {
        todo!()
    }

    fn sync_attributes(&self, _entry: &VendorEntry) -> Result<(), Error> {
        todo!()
    }
}
