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
        entry: &VendorEntry,
        merge: &VendorMerge,
    ) -> Result<(), Error> {
        use gix::bstr::ByteSlice as _;

        // Write the result tree (with conflict markers) to the working copy.
        self.checkout_vendor(entry, merge.result_tree)?;

        // Reopen the index we just wrote so we can splice in unmerged stages.
        let mut main_index = self.open_index().map_err(|e| Error::Gix(Box::new(e)))?;
        main_index.set_path(self.git_dir().join("index"));

        for conflict in &merge.conflicts {
            let path_bytes = gix::bstr::BString::from(conflict.path.as_bytes());
            let path_bstr = path_bytes.as_bstr();

            // Remove the stage-0 entry for this path.
            main_index.remove_entries(|_, p, _| p == path_bstr);

            // Insert stage 1/2/3 entries for each present stage.
            for (stage_idx, stage_variant) in [
                (0usize, gix::index::entry::Stage::Base),
                (1usize, gix::index::entry::Stage::Ours),
                (2usize, gix::index::entry::Stage::Theirs),
            ] {
                if let Some((tree_mode, oid)) = conflict.stages[stage_idx] {
                    let mode = gix::index::entry::Mode::from(tree_mode);
                    let flags = gix::index::entry::Flags::from_stage(stage_variant);
                    main_index.dangerously_push_entry(
                        gix::index::entry::Stat::default(),
                        oid,
                        flags,
                        mode,
                        path_bstr,
                    );
                }
            }
        }

        main_index.sort_entries();
        main_index
            .write(gix::index::write::Options::default())
            .map_err(|e| Error::Gix(Box::new(e)))?;

        Ok(())
    }

    fn track_vendor(&self, entry: &VendorEntry, paths: &[&str]) -> Result<(), Error> {
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;
        let gitattributes = workdir.join(".gitattributes");

        let existing = if gitattributes.exists() {
            std::fs::read_to_string(&gitattributes)?
        } else {
            String::new()
        };

        let attr_value = format!("vendor={}", entry.name.as_str());

        // Collect lines already attributed to this vendor for idempotency.
        let already_tracked: std::collections::HashSet<&str> = existing
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, ' ');
                let path = parts.next()?;
                let attr = parts.next()?.trim();
                if attr == attr_value { Some(path) } else { None }
            })
            .collect();

        let mut out = existing.clone();
        // Ensure existing content ends with a newline before appending.
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for path in paths {
            if !already_tracked.contains(path) {
                out.push_str(path);
                out.push(' ');
                out.push_str(&attr_value);
                out.push('\n');
            }
        }

        if out != existing {
            std::fs::write(&gitattributes, out.as_bytes())?;
        }

        Ok(())
    }

    fn untrack_vendor(&self, entry: &VendorEntry, paths: &[&str]) -> Result<(), Error> {
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;
        let gitattributes = workdir.join(".gitattributes");

        if !gitattributes.exists() {
            return Ok(());
        }

        let existing = std::fs::read_to_string(&gitattributes)?;
        let attr_value = format!("vendor={}", entry.name.as_str());

        let remove: std::collections::HashSet<&str> = paths.iter().copied().collect();

        let filtered: String = existing
            .lines()
            .filter(|line| {
                let mut parts = line.splitn(2, ' ');
                let path = parts.next().unwrap_or("");
                let attr = parts.next().map(str::trim).unwrap_or("");
                !(attr == attr_value && remove.contains(path))
            })
            .flat_map(|line| [line, "\n"])
            .collect();

        if filtered != existing {
            std::fs::write(&gitattributes, filtered.as_bytes())?;
        }

        Ok(())
    }

    fn sync_attributes(&self, _entry: &VendorEntry) -> Result<(), Error> {
        todo!()
    }
}
