//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub use error::Error;
use gix::bstr::{BStr, ByteSlice as _};
use gix::remote::fetch::{Status, refs::update::Mode};
pub use vendor::{
    ConflictStages, PatternMapping, VendorConfig, VendorEntry, VendorMerge, VendorMode, VendorName,
    VendorRepository, VendorStatus, VendorWorktree,
};

/// Whether a synthesized local path is unsafe to write into the result tree.
///
/// Splits on `/` and validates every component with [`gix::validate::path::component`]
/// — gix's analogue of git's `verify_path` — so empty components (leading or
/// trailing `/`, `//`), `.`/`..`, and `.git` (including HFS/NTFS-obfuscated
/// forms) are all rejected. Applied here, the boundary where untrusted
/// `destination` config first becomes a tree path. Default `Options` keep
/// every protection on so the result tree stays checkout-safe on any OS;
/// `mode: None` because symlink-leaf handling is the checkout boundary's job
/// ([`VendorWorktree`]).
fn is_unsafe_local_path(path: &gix::bstr::BStr) -> bool {
    path.split_str("/").any(|comp| {
        gix::validate::path::component(
            comp.into(),
            None,
            gix::validate::path::component::Options::default(),
        )
        .is_err()
    })
}

impl VendorRepository for gix::Repository {
    /// Fetches `entry.tracking_ref()` from `entry.url` into `refs/vendor/<name>`
    /// and returns the *peeled* tip OID. `refs/vendor/<name>` is always written
    /// to point directly at that same peeled OID, even when the tracked ref is
    /// an annotated tag — the tag object itself is never stored there.
    ///
    /// If the local ref is already up to date, the ref tip's existing object hash
    /// is returned.
    ///
    /// Only `https`, `http`, `ssh`, `git`, and `file` URL schemes are accepted.
    /// Git's [`ext::<cmd>`][ext] scheme executes an arbitrary shell command as the
    /// transport; any scheme not explicitly listed above is rejected to prevent
    /// that code-execution vector.
    ///
    /// [ext]: https://git-scm.com/docs/git-remote-ext
    fn fetch_vendor(&self, entry: &VendorEntry) -> Result<gix::ObjectId, Error> {
        let url = gix::url::parse(entry.url.as_str().into())
            .map_err(|e| Error::InvalidUrl(format!("{}: {e}", entry.url)))?;
        match url.scheme {
            gix::url::Scheme::Https
            | gix::url::Scheme::Http
            | gix::url::Scheme::Ssh
            | gix::url::Scheme::Git => {}
            gix::url::Scheme::File => {}
            ref other => {
                return Err(Error::InvalidUrl(format!(
                    "{}: refusing transport `{other:?}`; plug-in transports are not supported",
                    entry.url
                )));
            }
        }

        // The '+' indicates the local ref is force-updated as necessary.
        let refspec = format!("+{}:{}", entry.tracking_ref(), entry.vendor_ref());
        let remote = self
            .remote_at(entry.url.as_str())?
            .with_refspecs([refspec.as_str()], gix::remote::Direction::Fetch)?
            .with_fetch_tags(gix::remote::fetch::Tags::None);
        let outcome = remote
            .connect(gix::remote::Direction::Fetch)?
            .prepare_fetch(gix::progress::Discard, Default::default())?
            .receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
            .map_err(|e| match e {
                gix::remote::fetch::Error::NoMapping { .. } => Error::Fetch(format!(
                    "remote has no ref matching `{}` for vendor `{}`",
                    entry.tracking_ref(),
                    entry.name
                )),
                other => Error::Gix(Box::new(other)),
            })?;

        let update_refs = match &outcome.status {
            Status::NoPackReceived { update_refs, .. } | Status::Change { update_refs, .. } => {
                update_refs
            }
        };
        // gix records one update per refspec mapping that resolved, not one per
        // ref that moved: a no-op fetch (ref already current) still yields an
        // update with `Mode::NoChangeNeeded`, so `updates` is non-empty. It is
        // empty only when the refspec matched no remote ref at all, and that
        // case is already reported above as `fetch::Error::NoMapping`. This
        // guard is therefore defensive and unreachable for a real remote.
        if update_refs.updates.is_empty() {
            return Err(Error::Fetch(format!(
                "remote has no ref matching `{}` for vendor `{}`",
                entry.tracking_ref(),
                entry.name
            )));
        }
        for update in &update_refs.updates {
            if matches!(
                update.mode,
                Mode::RejectedSourceObjectNotFound { .. }
                    | Mode::RejectedTagUpdate
                    | Mode::RejectedNonFastForward
                    | Mode::RejectedToReplaceWithUnborn
                    | Mode::RejectedCurrentlyCheckedOut { .. }
            ) {
                return Err(Error::Fetch(format!(
                    "upstream ref `{}` for vendor `{}`: {}",
                    entry.tracking_ref(),
                    entry.name,
                    update.mode
                )));
            }
        }

        // Read the upstream OID from the refmap rather than by re-reading the
        // local vendor ref.  When the remote advertises HEAD as a symbolic ref
        // (e.g. `HEAD → refs/heads/main`) and the local repo happens to have a
        // branch of the same name, gix writes `refs/vendor/<name>` as a symref
        // pointing to that local branch; `peel_to_id()` would then silently
        // return the *local* HEAD instead of the upstream tip.  The refmap
        // carries the actual upstream OID directly before any local ref
        // resolution, so keying on it sidesteps the bug entirely.
        // Upstream: https://github.com/GitoxideLabs/gitoxide/issues/2613
        let vendor_ref = entry.vendor_ref();
        let id = outcome
            .ref_map
            .mappings
            .iter()
            .find(|m| {
                m.local
                    .as_deref()
                    .map(|l| l == vendor_ref.as_bytes())
                    .unwrap_or(false)
            })
            .and_then(|m| m.remote.peeled_id())
            .map(gix::oid::to_owned)
            .ok_or_else(|| {
                Error::Fetch(format!(
                    "remote has no ref matching `{}` for vendor `{}`",
                    entry.tracking_ref(),
                    entry.name
                ))
            })?;

        // `peeled_id()` only returns a peeled value when the ref advertisement
        // carried one; fetching an annotated tag by its own object SHA
        // (`Source::ObjectId`) has no such advertisement, so `id` may still be
        // the tag object itself. Peel explicitly so callers always get a commit.
        let id = self
            .find_object(id)
            .map_err(|e| Error::Gix(Box::new(e)))?
            .peel_to_commit()
            .map_err(|e| Error::Gix(Box::new(e)))?
            .id()
            .detach();

        // Force `refs/vendor/<name>` to point directly at `id`, overwriting
        // whatever gix wrote for it (see the gix#2613 note above: it may be a
        // symref into the local branch namespace rather than a direct ref to
        // the fetched commit). This keeps `vendor_tip`/`vendor_status`, which
        // read the ref directly, from resolving the corrupted symref.
        self.reference(
            entry.vendor_ref(),
            id,
            gix::refs::transaction::PreviousValue::Any,
            format!("fetch {}", entry.tracking_ref()),
        )
        .map_err(|e| Error::Gix(Box::new(e)))?;

        Ok(id)
    }

    fn vendor_tip(&self, entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error> {
        match self.try_find_reference(&entry.vendor_ref())? {
            None => Ok(None),
            Some(mut reference) => Ok(Some(reference.peel_to_id()?.detach())),
        }
    }

    fn vendor_status(&self, entry: &VendorEntry) -> Result<VendorStatus, Error> {
        let Some(mut reference) = self.try_find_reference(&entry.vendor_ref())? else {
            return Ok(VendorStatus::NotFetched);
        };
        let upstream = reference.peel_to_id()?.detach();

        // No recorded base: fetched but never merged, so the tip is the first
        // update to apply.
        let Some(base) = entry.base else {
            return Ok(VendorStatus::UpdateAvailable { upstream });
        };

        if upstream == base {
            return Ok(VendorStatus::UpToDate);
        }

        // The tip is an update iff `base` is an ancestor of it, i.e. the best
        // merge-base of the two equals `base`. Unrelated histories (no merge
        // base) mean upstream rewound or rewrote past `base`.
        match self.merge_base(base, upstream) {
            Ok(merge_base) if merge_base.detach() == base => {
                Ok(VendorStatus::UpdateAvailable { upstream })
            }
            Ok(_) | Err(gix::repository::merge_base::Error::NotFound { .. }) => {
                Ok(VendorStatus::ForcePushed { upstream })
            }
            Err(e) => Err(e.into()),
        }
    }

    fn upstream_tree(
        &self,
        entry: &VendorEntry,
        commit: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        // Pair each compiled glob with the `PatternMapping` it came from.
        // `gix_glob` rejects some globs (empty, blank, leading `#`); pairing
        // instead of collecting a parallel `Vec<Pattern>` makes a match index
        // structurally unable to desync from its remapping.
        let patterns: Vec<(gix_glob::Pattern, &PatternMapping)> = entry
            .patterns
            .iter()
            .filter_map(|p| gix_glob::Pattern::from_bytes(p.glob.as_bytes()).map(|pat| (pat, p)))
            .collect();

        // A non-empty config whose globs all fail to compile would otherwise
        // silently yield the empty tree and, through merge, delete the
        // vendor's content. Surface it instead.
        if !entry.patterns.is_empty() && patterns.is_empty() {
            return Err(Error::Config(format!(
                "vendor '{}': none of its {} configured pattern(s) are valid \
                 globs; check for empty or comment-only `pattern` values",
                entry.name,
                entry.patterns.len(),
            )));
        }

        let tree = self.find_commit(commit)?.tree()?;

        let mut editor = self.empty_tree().edit()?;
        for record in tree.traverse().breadthfirst.files()? {
            if record.mode.is_tree() {
                continue;
            }
            // Match and remap on the raw path bytes: git tree paths are
            // arbitrary bytes, and a lossy UTF-8 round-trip would both
            // misdirect glob matching and silently rename (or collide)
            // non-UTF-8 files in the result tree.
            let upstream_path = record.filepath.as_bstr();
            let basename_pos = upstream_path.rfind_byte(b'/').map(|i| i + 1);
            let Some((_, mapping)) = patterns.iter().find(|(pat, _)| {
                pat.matches_repo_relative_path(
                    upstream_path,
                    basename_pos,
                    Some(false),
                    gix_glob::pattern::Case::Sensitive,
                    gix_glob::wildmatch::Mode::NO_MATCH_SLASH_LITERAL,
                )
            }) else {
                continue;
            };
            let Some(local_path) = mapping.local_path(upstream_path) else {
                continue;
            };
            // A metachar-free or exact-file glob has a literal prefix equal to
            // the whole glob, so `local_path` strips it to `""` (and a
            // `dir/`-style destination leaves a trailing empty component).
            // Passing that to the tree editor yields gix's opaque
            // `EmptyPathComponent`. The same guard rejects synthesized paths
            // that would escape the tree (`..`, absolute, or a `.git`
            // component): `destination` is untrusted config and this is the
            // boundary where it becomes a local path, mirroring git's own
            // `verify_path`. Surface either as an actionable config error.
            if is_unsafe_local_path(local_path.as_bstr()) {
                return Err(Error::Config(format!(
                    "vendor '{name}': pattern '{glob}' maps upstream \
                     '{upstream_path}' to the invalid local path \
                     '{local_path}'; give the pattern a destination with a \
                     file name, e.g. '{glob}:vendor/{upstream_path}'",
                    name = entry.name,
                    glob = mapping.glob,
                )));
            }
            editor.upsert(local_path.as_bstr(), record.mode.kind(), record.oid)?;
        }

        Ok(editor.write()?.detach())
    }

    fn base_tree(&self, entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error> {
        let Some(base) = entry.base else {
            return Ok(None);
        };
        self.upstream_tree(entry, base).map(Some)
    }

    fn ours_tree(&self, entry: &VendorEntry, ours: gix::ObjectId) -> Result<gix::ObjectId, Error> {
        let selected: std::collections::BTreeSet<gix::bstr::BString> =
            resolve_vendor_paths(self, entry, ours)?
                .into_iter()
                .collect();

        let tree = self.find_commit(ours)?.tree()?;
        let mut editor = self.empty_tree().edit()?;
        for record in tree.traverse().breadthfirst.files()? {
            if record.mode.is_tree() {
                continue;
            }
            // Select and write on the raw path bytes, never a lossy UTF-8
            // round-trip: git tree paths are arbitrary bytes, and `to_str_lossy`
            // would both rename non-UTF-8 paths in the result tree and collapse
            // distinct paths onto one another in the selection set, mirroring
            // the hazard `upstream_tree` documents.
            let path = record.filepath.as_bstr();
            if selected.contains(path) {
                editor.upsert(path, record.mode.kind(), record.oid)?;
            }
        }
        Ok(editor.write()?.detach())
    }

    fn vendor_paths(
        &self,
        entry: &VendorEntry,
        ours: gix::ObjectId,
    ) -> Result<Vec<gix::bstr::BString>, Error> {
        resolve_vendor_paths(self, entry, ours)
    }

    /// `theirs` is an *upstream* commit; its pattern-filtered, remapped tree is
    /// the "theirs" side. `ours` is a *local* commit; its attribute-filtered
    /// tree is the "ours" side. The ancestor is [`base_tree`](Self::base_tree)
    /// — the empty tree before the first merge, which turns this into an "add".
    /// All three trees live in local path space, so reported conflict paths and
    /// the result tree are local.
    ///
    /// gix's default blob policy gives exactly the documented resolution: a
    /// textual conflict is written with conflict markers, and a binary conflict
    /// keeps the "ours" (local) blob; both are still listed in `conflicts`. No
    /// auto-resolution strategy is applied, so a real conflict is never
    /// silently dropped from the result.
    fn merge_vendor(
        &self,
        entry: &VendorEntry,
        ours: gix::ObjectId,
        theirs: gix::ObjectId,
    ) -> Result<VendorMerge, Error> {
        let our_tree = self.ours_tree(entry, ours)?;
        let their_tree = self.upstream_tree(entry, theirs)?;
        let ancestor_tree = self.base_tree(entry)?;
        // `None` ancestor means no common history (first add): merge against
        // the empty tree, but report `ancestor_tree: None` per the contract.
        let ancestor =
            ancestor_tree.unwrap_or_else(|| gix::ObjectId::empty_tree(self.object_hash()));

        let labels = gix::merge::blob::builtin_driver::text::Labels {
            ancestor: Some(b"base".as_bstr()),
            current: Some(b"ours".as_bstr()),
            other: Some(b"theirs".as_bstr()),
        };
        let options = self.tree_merge_options()?;
        let mut outcome = self.merge_trees(ancestor, our_tree, their_tree, labels, options)?;

        let result_tree = outcome.tree.write()?.detach();

        // Don't reconstruct the conflicted state by hand: build the index git
        // itself would leave, then read it back. Starting from the result tree
        // and applying the conflicting stages (using git's own notion of
        // "unresolved" — markers or an undecidable tree merge, the default
        // `TreatAsUnresolved`) yields the canonical unmerged index. It is keyed
        // by `(path, stage)` with at most one entry per stage, so deduplication
        // is git's, not ours: a single path that surfaces as several `Conflict`s
        // (e.g. rename/rename onto one destination) still collapses to one
        // stage 1/2/3 triple exactly as a stalled `git merge` would record it.
        let how = gix::merge::tree::TreatAsUnresolved::git();
        let mut index = self.index_from_tree(&result_tree)?;
        outcome.index_changed_after_applying_conflicts(
            &mut index,
            how,
            gix::merge::tree::apply_index_entries::RemovalMode::Prune,
        );

        use gix::index::entry::Stage;
        let mut by_path: std::collections::BTreeMap<
            gix::bstr::BString,
            [Option<(gix::objs::tree::EntryMode, gix::ObjectId)>; 3],
        > = std::collections::BTreeMap::new();
        for e in index.entries() {
            let stage = match e.stage() {
                Stage::Base => 0,
                Stage::Ours => 1,
                Stage::Theirs => 2,
                Stage::Unconflicted => continue,
            };
            if let Some(mode) = e.mode.to_tree_entry_mode() {
                by_path.entry(e.path(&index).to_owned()).or_default()[stage] = Some((mode, e.id));
            }
        }
        let conflicts = by_path
            .into_iter()
            .map(|(path, stages)| crate::ConflictStages {
                path: path.to_str_lossy().into_owned(),
                stages,
            })
            .collect();

        Ok(VendorMerge {
            upstream_commit: theirs,
            ancestor_tree,
            result_tree,
            conflicts,
        })
    }

    fn vendor_overlay(
        &self,
        entry: &VendorEntry,
        base_commit: gix::ObjectId,
        vendor_tree: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        let mut editor = self.find_commit(base_commit)?.tree()?.edit()?;
        for path in resolve_vendor_paths(self, entry, base_commit)? {
            editor.remove(path.as_bstr())?;
        }
        let result = self.find_object(vendor_tree)?.into_tree();
        for record in result.traverse().breadthfirst.files()? {
            if record.mode.is_tree() {
                continue;
            }
            editor.upsert(record.filepath.as_bstr(), record.mode.kind(), record.oid)?;
        }
        Ok(editor.write()?.detach())
    }

    fn commit_vendor<'a, 'c>(
        &self,
        committer: impl Into<gix::actor::SignatureRef<'c>>,
        author: impl Into<gix::actor::SignatureRef<'a>>,
        message: impl AsRef<str>,
        entry: &VendorEntry,
        parent: gix::ObjectId,
        merge: &VendorMerge,
    ) -> Result<gix::ObjectId, Error> {
        if merge.has_conflicts() {
            let paths = merge
                .conflicts
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::Conflict(format!(
                "vendor '{}' has {} unresolved conflict(s): {paths}; resolve them \
                 in the working tree before committing",
                entry.name,
                merge.conflicts.len(),
            )));
        }

        let committer: gix::actor::SignatureRef<'c> = committer.into();
        let author: gix::actor::SignatureRef<'a> = author.into();

        // The second parent records the integrated upstream point. By default
        // it is `merge.upstream_commit` itself — a real edge into the
        // `refs/vendor/<name>` graph, so the upstream's full history is
        // reachable from `HEAD`. In squash mode it is instead a fresh,
        // parentless commit holding only the remapped vendor tree, severing
        // that reachability so a plain clone stays thin; the integrated
        // upstream OID is recorded in its message so the squashed-from point
        // survives a clone lacking the upstream history.
        let second_parent = if entry.mode == VendorMode::Squash {
            let squash = gix::objs::Commit {
                tree: self.upstream_tree(entry, merge.upstream_commit)?,
                parents: Default::default(),
                // Shares the merge commit's author/committer identity and
                // timestamp: the two objects record a single operation.
                author: author.into(),
                committer: committer.into(),
                encoding: None,
                // The squashed-from upstream OID is recorded as a git trailer so
                // it survives a thin clone (which lacks the upstream graph) and
                // stays machine-readable, while riding in the message — which
                // every tool preserves — rather than a fragile extra header.
                message: format!(
                    "squash: vendor '{}'\n\nSquashed-upstream: {}\n",
                    entry.name, merge.upstream_commit
                )
                .into(),
                extra_headers: Vec::new(),
            };
            self.write_object(&squash)?.detach()
        } else {
            merge.upstream_commit
        };

        let full_tree = self.vendor_overlay(entry, parent, merge.result_tree)?;

        let commit = gix::objs::Commit {
            tree: full_tree,
            // First parent is the local "ours" commit; second records the
            // integrated upstream point (see `second_parent` above).
            parents: [parent, second_parent].into_iter().collect(),
            author: author.into(),
            committer: committer.into(),
            encoding: None,
            message: message.as_ref().into(),
            extra_headers: Vec::new(),
        };
        Ok(self.write_object(&commit)?.detach())
    }
}

/// The local paths in `ours`'s tree carrying the `vendor=<entry.name>`
/// attribute — the local-side content filter that
/// [`ours_tree`](VendorRepository::ours_tree) restricts to.
///
/// `.gitattributes` are resolved from the commit's tree through the object
/// database (`Source::IdMapping`), never the working copy, so nested
/// `.gitattributes` and last-match-wins precedence are honored exactly as git
/// resolves them. Only an explicit `vendor=<name>` value selects a path: a
/// bare `vendor`, `-vendor`, or a different value does not.
fn resolve_vendor_paths(
    repo: &gix::Repository,
    entry: &VendorEntry,
    ours: gix::ObjectId,
) -> Result<Vec<gix::bstr::BString>, Error> {
    let tree = repo.find_commit(ours)?.tree()?;
    let tree_id = tree.id().detach();
    let index = repo.index_from_tree(&tree_id)?;
    let mut stack = repo.attributes_only(
        &index,
        gix::worktree::stack::state::attributes::Source::IdMapping,
    )?;
    let mut outcome = stack.selected_attribute_matches(["vendor"]);

    let mut paths = Vec::new();
    for record in tree.traverse().breadthfirst.files()? {
        if record.mode.is_tree() {
            continue;
        }
        let platform = stack.at_entry(record.filepath.as_bstr(), None)?;
        outcome.reset();
        platform.matching_attributes(&mut outcome);
        let is_ours = outcome.iter_selected().any(|m| {
            matches!(
                m.assignment.state,
                gix::attrs::StateRef::Value(v)
                    if v.as_bstr() == entry.name.as_bytes().as_bstr()
            )
        });
        if is_ours {
            paths.push(record.filepath);
        }
    }
    Ok(paths)
}

/// Like [`resolve_vendor_paths`], but resolves against the current on-disk
/// index instead of a commit's tree. For use on an unborn `HEAD`, where
/// staged entries exist (e.g. right after `add`) but there is no commit yet
/// to read a tree from. Index entries are always files, so no tree-vs-blob
/// filtering is needed.
pub fn resolve_vendor_paths_uncommitted(
    repo: &gix::Repository,
    entry: &VendorEntry,
) -> Result<Vec<gix::bstr::BString>, Error> {
    let index = repo.open_index().map_err(|e| Error::Gix(Box::new(e)))?;
    let mut stack = repo.attributes_only(
        &index,
        gix::worktree::stack::state::attributes::Source::IdMapping,
    )?;
    let mut outcome = stack.selected_attribute_matches(["vendor"]);

    let mut paths = Vec::new();
    for e in index.entries() {
        let path = e.path(&index).to_owned();
        let platform = stack.at_entry(path.as_bstr(), None)?;
        outcome.reset();
        platform.matching_attributes(&mut outcome);
        let is_ours = outcome.iter_selected().any(|m| {
            matches!(
                m.assignment.state,
                gix::attrs::StateRef::Value(v)
                    if v.as_bstr() == entry.name.as_bytes().as_bstr()
            )
        });
        if is_ours {
            paths.push(path);
        }
    }
    Ok(paths)
}

// ── worktree impl ────────────────────────────────────────────────────────────

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
            .and_then(|id| resolve_vendor_paths(self, entry, id).ok())
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

        // The tree returned to callers still overlays the vendor tree onto the
        // full HEAD tree (an unborn HEAD has no base commit, so the vendor
        // tree is itself the whole tree) — callers need this OID to mint
        // commits regardless of what the on-disk index looks like.
        let full_tree = match head_id {
            Some(id) => self.vendor_overlay(entry, id, tree)?,
            None => tree,
        };

        // The on-disk index, however, must not be derived from `full_tree`:
        // that overlay reads from HEAD, never the index, so any entry staged
        // but not yet committed (an addition or modification) would be
        // silently dropped. Instead, start from the actual current index —
        // or an empty one if none exists yet — and surgically apply only this
        // vendor's own path changes, leaving every other entry untouched.
        let mut main_index = match self.open_index() {
            Ok(idx) => idx,
            Err(_) => self.index_from_tree(&gix::ObjectId::empty_tree(self.object_hash()))?,
        };
        main_index.set_path(self.git_dir().join("index"));

        for removed in old_paths.difference(&new_paths) {
            main_index.remove_entries(|_, p, _| p == removed.as_bstr());
        }
        for e in vendor_index.entries() {
            let path = e.path(&vendor_index).to_owned();
            main_index.remove_entries(|_, p, _| p == path.as_bstr());
            main_index.dangerously_push_entry(e.stat, e.id, e.flags, e.mode, path.as_bstr());
        }
        main_index.sort_entries();
        // `remove_entries`/`dangerously_push_entry` don't update the index's
        // cached-tree extension, so a stale one would make a native `git
        // commit` skip rehashing changed subtrees and record the wrong tree.
        main_index.remove_tree();

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

        self.checkout_vendor(entry, merge.result_tree)?;

        // Reopen the index we just wrote so we can splice in unmerged stages.
        let mut main_index = self.open_index().map_err(|e| Error::Gix(Box::new(e)))?;
        main_index.set_path(self.git_dir().join("index"));

        for conflict in &merge.conflicts {
            let path_bytes = gix::bstr::BString::from(conflict.path.as_bytes());
            let path_bstr = path_bytes.as_bstr();

            main_index.remove_entries(|_, p, _| p == path_bstr);

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

    fn track_vendor(&self, entry: &VendorEntry, paths: &[&BStr]) -> Result<gix::ObjectId, Error> {
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
                    Some(unescape_attr_pattern(&pattern))
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
                out.extend_from_slice(&escape_attr_pattern(path.as_bytes()));
                out.push(b' ');
                out.extend_from_slice(attr_bytes);
                out.push(b'\n');
            }
        }

        if out != existing {
            std::fs::write(&gitattributes, &out)?;
        }

        stage_gitattributes(self, &out)
    }

    fn untrack_vendor(
        &self,
        entry: &VendorEntry,
        paths: &[&BStr],
    ) -> Result<Option<gix::ObjectId>, Error> {
        let workdir = self.workdir().ok_or(Error::NoWorkdir)?;
        let gitattributes = workdir.join(".gitattributes");

        if !gitattributes.exists() {
            return Ok(None);
        }

        let existing: Vec<u8> = std::fs::read(&gitattributes)?;
        let attr_value = format!("vendor={}", entry.name.as_str());
        let attr_bytes = attr_value.as_bytes();

        let remove: std::collections::HashSet<&[u8]> = paths.iter().map(|b| b.as_bytes()).collect();

        let mut filtered: Vec<u8> = Vec::with_capacity(existing.len());
        for line in existing.lines() {
            let keep = match split_attr_line(line) {
                Some((pattern, attr)) => {
                    !(attr == attr_bytes
                        && remove.contains(unescape_attr_pattern(&pattern).as_slice()))
                }
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

        Ok(Some(stage_gitattributes(self, &filtered)?))
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

/// Check that every path in `paths` can be written as a plain (unquoted)
/// `.gitattributes` pattern. Callers that will later checkout files and
/// mutate the working tree/index should validate paths with this *before*
/// doing so, so an invalid path aborts cleanly instead of leaving a
/// half-applied checkout behind — see [`check_attr_pattern`] for what's
/// rejected.
pub fn validate_trackable_paths(paths: &[&BStr]) -> Result<(), Error> {
    for p in paths {
        check_attr_pattern(p.as_bytes())?;
    }
    Ok(())
}

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

/// Escape glob metacharacters (`*`, `?`, `[`) and a pattern-initial `!` or `#`
/// with a backslash, so `path` matches only itself as a `.gitattributes`
/// pattern. `check_attr_pattern` already rejects a raw `\` in the input, so
/// every backslash in the result is unambiguously one we inserted here.
fn escape_attr_pattern(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len());
    for (i, &b) in path.iter().enumerate() {
        if matches!(b, b'*' | b'?' | b'[') || (i == 0 && matches!(b, b'!' | b'#')) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// Inverse of [`escape_attr_pattern`].
fn unescape_attr_pattern(pattern: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pattern.len());
    let mut i = 0;
    while i < pattern.len() {
        if pattern[i] == b'\\'
            && i + 1 < pattern.len()
            && (matches!(pattern[i + 1], b'*' | b'?' | b'[')
                || (i == 0 && matches!(pattern[i + 1], b'!' | b'#')))
        {
            i += 1;
        }
        out.push(pattern[i]);
        i += 1;
    }
    out
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
            return None;
        }
        Some((
            std::borrow::Cow::Borrowed(&line[..pos]),
            line[pos + 1..].trim(),
        ))
    }
}

#[cfg(test)]
#[path = "attr_tests.rs"]
mod tests;

/// Write `content` as a blob into the object database, upsert the
/// `.gitattributes` index entry to point at it, and return the blob OID.
fn stage_gitattributes(repo: &gix::Repository, content: &[u8]) -> Result<gix::ObjectId, Error> {
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

    Ok(blob_oid)
}
