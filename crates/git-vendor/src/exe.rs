//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use crate::{Error, VendorEntry, VendorMerge, VendorRepository, VendorWorktree};

impl VendorWorktree for gix::Repository {
    fn checkout_vendor(&self, entry: &VendorEntry, tree: gix::ObjectId) -> Result<(), Error> {
        // SAFETY
        // This is the trust boundary where upstream content (carried verbatim
        // through `upstream_tree`, including symlink and gitlink modes,
        // mirroring git-subtree/submodule) reaches the working copy. Like
        // core git's `verify_path`/checkout, projection MUST refuse to write
        // through a symlinked leading path and reject `..`/absolute
        // components — use gix-worktree's checked checkout, never naive
        // `std::fs` writes. See the `upstream_tree` adversarial review (#5).
        // NOTE
        // Path-traversal safety (e.g. `../` components, symlinked leading
        // paths) is delegated to gix and is not covered by automated tests.
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;

        let head_id = self.head_commit().ok().map(|c| c.id().detach());

        let old_paths: std::collections::BTreeSet<gix::bstr::BString> = head_id
            .and_then(|id| crate::resolve_vendor_paths(self, entry, id).ok())
            .into_iter()
            .flatten()
            .collect();

        let mut vendor_index = self.index_from_tree(&tree)?;

        let new_paths: std::collections::BTreeSet<gix::bstr::BString> = vendor_index
            .entries()
            .iter()
            .map(|e| e.path(&vendor_index).to_owned())
            .collect();

        let opts = self
            .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
            .map_err(|e| Error::Gix(Box::new(e)))?;
        let progress = gix::progress::Discard;
        gix::worktree::state::checkout(
            &mut vendor_index,
            workdir,
            self.objects.clone().into_arc().map_err(Error::Io)?,
            &progress,
            &progress,
            &gix::interrupt::IS_INTERRUPTED,
            gix::worktree::state::checkout::Options {
                overwrite_existing: true,
                ..opts
            },
        )
        .map_err(|e| Error::Gix(Box::new(e)))?;

        for removed in old_paths.difference(&new_paths) {
            let abs = workdir.join(gix::path::from_bstr(removed).as_ref());
            if abs.symlink_metadata().is_ok() {
                std::fs::remove_file(&abs)?;
            }
        }

        // Overlay the vendor tree onto the full HEAD tree and rebuild the index
        // from the result. An unborn HEAD has no base commit, so the vendor tree
        // is itself the whole tree.
        let full_tree = match head_id {
            Some(id) => self.vendor_overlay(entry, id, tree)?,
            None => tree,
        };
        let mut main_index = self.index_from_tree(&full_tree)?;
        main_index.set_path(self.git_dir().join("index"));

        // `index_from_tree` zeroes stat data; carry over the stats checkout just
        // populated on the vendor entries so `git status` need not re-hash them.
        let vendor_stats: std::collections::HashMap<gix::bstr::BString, gix::index::entry::Stat> =
            vendor_index
                .entries()
                .iter()
                .map(|e| (e.path(&vendor_index).to_owned(), e.stat))
                .collect();
        for (e, path) in main_index.entries_mut_with_paths() {
            if let Some(stat) = vendor_stats.get(path) {
                e.stat = *stat;
            }
        }

        main_index
            .write(gix::index::write::Options::default())
            .map_err(|e| Error::Gix(Box::new(e)))?;

        Ok(())
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
