//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use crate::{Error, VendorEntry, VendorMerge};

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

    /// Project a conflicted merge onto the working copy for manual resolution.
    ///
    /// Writes `merge.result_tree` — which carries conflict markers in the
    /// textual blobs and the "ours" blob for binary conflicts — to the vendor's
    /// paths, and records each [`merge.conflicts`](VendorMerge::conflicts) entry
    /// as unmerged in the index from its stage 1/2/3 versions, leaving the same
    /// state a stalled `git merge` does: `git status` shows the conflicts and
    /// `git add` resolves them.
    ///
    /// Use this instead of
    /// [`commit_vendor`](crate::VendorRepository::commit_vendor), which refuses a
    /// conflicted [`VendorMerge`] with [`Error::Conflict`]: the conflicted state
    /// is resolved in the working tree and committed through ordinary git, never
    /// minted directly with markers baked in.
    fn checkout_vendor_conflicted(
        &self,
        entry: &VendorEntry,
        merge: &VendorMerge,
    ) -> Result<(), Error>;

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
