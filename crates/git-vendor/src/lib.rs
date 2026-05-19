//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub mod exe;

pub use error::Error;
pub use exe::VendorWorktree;
use gix::bstr::ByteSlice as _;
use gix::remote::fetch::{Status, refs::update::Mode};
pub use vendor::{
    PatternMapping, VendorConfig, VendorEntry, VendorMerge, VendorName, VendorRepository,
    VendorStatus,
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
            | gix::url::Scheme::Git
            | gix::url::Scheme::File => {}
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

    fn vendor_status(&self, _entry: &VendorEntry) -> Result<VendorStatus, Error> {
        todo!()
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
        _entry: &VendorEntry,
        _ours: gix::ObjectId,
    ) -> Result<Vec<String>, Error> {
        todo!()
    }

    fn merge_vendor(
        &self,
        _entry: &VendorEntry,
        _ours: gix::ObjectId,
        _theirs: gix::ObjectId,
    ) -> Result<VendorMerge, Error> {
        todo!()
    }

    fn commit_vendor(
        &self,
        _entry: &VendorEntry,
        _parent: gix::ObjectId,
        _merge: &VendorMerge,
    ) -> Result<gix::ObjectId, Error> {
        todo!()
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
