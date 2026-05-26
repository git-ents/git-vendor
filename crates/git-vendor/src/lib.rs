//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub mod exe;

pub use error::Error;
use gix::bstr::ByteSlice as _;
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
    /// and returns the *peeled* tip OID. When the tracked ref is an annotated
    /// tag, the returned id is the tag's ultimate target, not the tag object
    /// stored at `refs/vendor/<name>`.
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
            gix::url::Scheme::File => {
                if !self.is_bare() {
                    return Err(Error::InvalidUrl(format!(
                        "{}: refusing transport `{:?}`; local transports are not yet supported",
                        entry.url, url.scheme
                    )));
                }
            }
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

        let mut reference = self.find_reference(&entry.vendor_ref())?;
        let id = reference.peel_to_id()?.detach();
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

        // Splice the vendor-only result_tree into the full parent tree: remove
        // old vendor paths (they may have changed names or disappeared) then
        // upsert every entry from the merge result. This produces a root tree
        // that carries the merged vendor content alongside all non-vendored files.
        let full_tree = {
            let mut editor = self.find_commit(parent)?.tree()?.edit()?;
            for path in resolve_vendor_paths(self, entry, parent)? {
                editor.remove(path.as_bstr())?;
            }
            let result = self.find_object(merge.result_tree)?.into_tree();
            for record in result.traverse().breadthfirst.files()? {
                if record.mode.is_tree() {
                    continue;
                }
                editor.upsert(record.filepath.as_bstr(), record.mode.kind(), record.oid)?;
            }
            editor.write()?.detach()
        };

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
