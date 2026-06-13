//! Working-copy projection layer: the side-effecting half of vendoring.
//!
//! Where [`VendorRepository`](crate::VendorRepository) is a pure
//! object-database algebra, every method here writes the index and working
//! tree (including tracked files like `.gitattributes`). It is the sole owner
//! of the one working copy and its ambient `HEAD`/index state, kept distinct
//! from the pure object-database operations.

use gix::bstr::{BStr, ByteSlice as _};

use crate::{Error, VendorEntry, VendorMerge, VendorMode, VendorRepository, VendorWorktree};

impl VendorWorktree for gix::Repository {
    fn checkout_vendor(
        &self,
        entry: &VendorEntry,
        tree: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        // IMPORTANT
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

        Ok(full_tree)
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

    fn track_vendor(&self, entry: &VendorEntry, paths: &[&BStr]) -> Result<(), Error> {
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;
        let gitattributes = workdir.join(".gitattributes");

        let existing: Vec<u8> = if gitattributes.exists() {
            std::fs::read(&gitattributes)?
        } else {
            Vec::new()
        };

        let attr_value = format!("vendor={}", entry.name.as_str());
        let attr_bytes = attr_value.as_bytes();

        for path in paths {
            check_attr_pattern(path.as_bytes())?;
        }

        let already_tracked: std::collections::HashSet<Vec<u8>> = existing
            .lines()
            .filter_map(|line| {
                let (pattern, attr) = split_attr_line(line)?;
                if attr == attr_bytes {
                    Some(pattern.into_owned())
                } else {
                    None
                }
            })
            .collect();

        let mut out = existing.clone();
        if !out.is_empty() && out.last() != Some(&b'\n') {
            out.push(b'\n');
        }
        for path in paths {
            if !already_tracked.contains(path.as_bytes()) {
                out.extend_from_slice(path.as_bytes());
                out.push(b' ');
                out.extend_from_slice(attr_bytes);
                out.push(b'\n');
            }
        }

        if out != existing {
            std::fs::write(&gitattributes, &out)?;
        }

        stage_gitattributes(self, &out)?;
        Ok(())
    }

    fn untrack_vendor(&self, entry: &VendorEntry, paths: &[&BStr]) -> Result<(), Error> {
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;
        let gitattributes = workdir.join(".gitattributes");

        if !gitattributes.exists() {
            return Ok(());
        }

        let existing: Vec<u8> = std::fs::read(&gitattributes)?;
        let attr_value = format!("vendor={}", entry.name.as_str());
        let attr_bytes = attr_value.as_bytes();

        let remove: std::collections::HashSet<&[u8]> = paths.iter().map(|b| b.as_bytes()).collect();

        let mut filtered: Vec<u8> = Vec::with_capacity(existing.len());
        for line in existing.lines() {
            let keep = match split_attr_line(line) {
                Some((pattern, attr)) => !(attr == attr_bytes && remove.contains(pattern.as_ref())),
                None => true,
            };
            if keep {
                filtered.extend_from_slice(line);
                filtered.push(b'\n');
            }
        }

        if filtered != existing {
            std::fs::write(&gitattributes, &filtered)?;
        }

        stage_gitattributes(self, &filtered)?;
        Ok(())
    }

    fn prepare_merge(
        &self,
        entry: &VendorEntry,
        merge: &VendorMerge,
        message: &str,
    ) -> Result<(), Error> {
        let git_dir = self.git_dir();
        if entry.mode == VendorMode::Squash {
            std::fs::write(git_dir.join("SQUASH_MSG"), message.as_bytes())?;
        } else {
            std::fs::write(
                git_dir.join("MERGE_HEAD"),
                format!("{}\n", merge.upstream_commit),
            )?;
            std::fs::write(git_dir.join("MERGE_MSG"), message.as_bytes())?;
        }
        Ok(())
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Return `Err` if `path` contains characters that require C-style quoting in
/// `.gitattributes` (space, tab, `#`, `"`, `\`, or control characters).
/// Git source paths from tree objects never contain these in practice.
fn check_attr_pattern(path: &[u8]) -> Result<(), Error> {
    let needs_quoting = |b: u8| b.is_ascii_control() || matches!(b, b' ' | b'#' | b'"' | b'\\');
    if path.iter().copied().any(needs_quoting) {
        return Err(Error::InvalidPath(
            String::from_utf8_lossy(path).into_owned(),
        ));
    }
    Ok(())
}

/// Parse one `.gitattributes` line into `(unquoted_pattern, trimmed_attrs)`.
///
/// Returns `None` for blank lines, comment lines, or lines with no attribute
/// separator.  Handles both plain and C-style-quoted patterns using
/// [`gix_quote::ansi_c::undo`].
fn split_attr_line(line: &[u8]) -> Option<(std::borrow::Cow<'_, [u8]>, &[u8])> {
    if line.is_empty() || line[0] == b'#' {
        return None;
    }
    if line.starts_with(b"\"") {
        let (pattern, consumed) = gix_quote::ansi_c::undo(line.as_bstr()).ok()?;
        let rest = line.get(consumed..)?;
        if rest.first().is_some_and(|&b| b == b' ' || b == b'\t') {
            let owned: Vec<u8> = pattern.as_ref().to_vec();
            Some((std::borrow::Cow::Owned(owned), rest[1..].trim()))
        } else {
            None
        }
    } else {
        let pos = line.iter().position(|&b| b == b' ' || b == b'\t')?;
        if pos == 0 {
            return None; // leading whitespace — no valid pattern before the separator
        }
        Some((
            std::borrow::Cow::Borrowed(&line[..pos]),
            line[pos + 1..].trim(),
        ))
    }
}

#[cfg(test)]
#[path = "exe_tests.rs"]
mod tests;

/// Write `content` as a blob into the object database and upsert the
/// `.gitattributes` index entry to point at it.
///
/// This exists because [`VendorWorktree::track_vendor`] and
/// [`VendorWorktree::untrack_vendor`] write `.gitattributes` as a working-copy
/// side effect rather than folding it into the vendor tree before
/// `index_from_tree` runs. Ideally those methods would return a blob OID so
/// the caller could include `.gitattributes` in `full_tree` like any other
/// file, making this function unnecessary.
fn stage_gitattributes(repo: &gix::Repository, content: &[u8]) -> Result<(), Error> {
    let blob_oid = repo
        .write_object(gix::objs::BlobRef { data: content })?
        .detach();

    let mut index = repo.open_index().map_err(|e| Error::Gix(Box::new(e)))?;
    index.remove_entries(|_, path, _| path == b".gitattributes".as_bstr());
    index.dangerously_push_entry(
        gix::index::entry::Stat::default(),
        blob_oid,
        gix::index::entry::Flags::empty(),
        gix::index::entry::Mode::FILE,
        b".gitattributes".as_bstr(),
    );
    index.sort_entries();
    index
        .write(gix::index::write::Options::default())
        .map_err(|e| Error::Gix(Box::new(e)))?;

    Ok(())
}
