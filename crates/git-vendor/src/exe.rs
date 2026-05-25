//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use crate::{Error, VendorEntry, VendorMerge, VendorWorktree};

impl VendorWorktree for gix::Repository {
    fn checkout_vendor(&self, _entry: &VendorEntry, _tree: gix::ObjectId) -> Result<(), Error> {
        // SAFETY
        // This is the trust boundary where upstream content (carried verbatim
        // through `upstream_tree`, including symlink and gitlink modes,
        // mirroring git-subtree/submodule) reaches the working copy. Like
        // core git's `verify_path`/checkout, projection MUST refuse to write
        // through a symlinked leading path and reject `..`/absolute
        // components — use gix-worktree's checked checkout, never naive
        // `std::fs` writes. See the `upstream_tree` adversarial review (#5).
        todo!()
    }

    fn checkout_vendor_conflicted(
        &self,
        _entry: &VendorEntry,
        _merge: &VendorMerge,
    ) -> Result<(), Error> {
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
