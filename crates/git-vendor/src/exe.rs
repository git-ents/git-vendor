//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use crate::{Error, VendorEntry, VendorMerge, VendorWorktree};

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

        let old_paths: std::collections::BTreeSet<gix::bstr::BString> = self
            .head_commit()
            .ok()
            .and_then(|c| {
                let id = c.id().detach();
                crate::resolve_vendor_paths(self, entry, id).ok()
            })
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

        let mut main_index = if self.git_dir().join("index").exists() {
            self.open_index().map_err(|e| Error::Gix(Box::new(e)))?
        } else {
            // Unborn HEAD: no .git/index exists yet; start from empty.
            gix::index::File::from_state(
                gix::index::State::new(self.object_hash()),
                self.git_dir().join("index"),
            )
        };

        let all_managed: std::collections::BTreeSet<gix::bstr::BString> =
            old_paths.iter().chain(new_paths.iter()).cloned().collect();
        main_index.remove_entries(|_, path, _| all_managed.contains(path));

        for e in vendor_index.entries() {
            let path = e.path(&vendor_index);
            main_index.dangerously_push_entry(e.stat, e.id, e.flags, e.mode, path);
        }
        main_index.sort_entries();

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
