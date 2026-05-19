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
        let patterns = entry
            .patterns
            .iter()
            .map(|p| {
                gix_glob::Pattern::from_bytes(p.glob.as_bytes()).ok_or_else(|| {
                    Error::Config(format!(
                        "vendor '{}': invalid pattern '{}'",
                        entry.name, p.glob
                    ))
                })
            })
            .collect::<Result<Vec<gix_glob::Pattern>, _>>()?;

        let tree = self.find_commit(commit)?.tree()?;

        let mut editor = self.empty_tree().edit()?;
        for record in tree.traverse().breadthfirst.files()? {
            if record.mode.is_tree() {
                continue;
            }
            let upstream_path = record.filepath.to_str_lossy();
            let path_bstr: &gix::bstr::BStr = gix::bstr::BStr::new(upstream_path.as_bytes());
            let basename_pos = upstream_path.rfind('/').map(|i| i + 1);
            let Some(i) = patterns.iter().position(|pat| {
                pat.matches_repo_relative_path(
                    path_bstr,
                    basename_pos,
                    Some(false),
                    gix_glob::pattern::Case::Sensitive,
                    gix_glob::wildmatch::Mode::NO_MATCH_SLASH_LITERAL,
                )
            }) else {
                continue;
            };
            let Some(local_path) = entry.patterns[i].local_path(&upstream_path) else {
                continue;
            };
            // A metachar-free or exact-file glob has a literal prefix equal to
            // the whole glob, so `local_path` strips it to `""` (and a
            // `dir/`-style destination leaves a trailing empty component).
            // Passing that to the tree editor yields gix's opaque
            // `EmptyPathComponent`; surface it as an actionable config error
            // instead, naming the file so the user can add a destination.
            if local_path.is_empty()
                || local_path.starts_with('/')
                || local_path.ends_with('/')
                || local_path.contains("//")
            {
                return Err(Error::Config(format!(
                    "vendor '{}': pattern '{}' maps upstream '{upstream_path}' to the \
                     invalid local path '{local_path}'; give the pattern a destination \
                     with a file name, e.g. '{}:vendor/{upstream_path}'",
                    entry.name, entry.patterns[i].glob, entry.patterns[i].glob,
                )));
            }
            editor.upsert(local_path, record.mode.kind(), record.oid)?;
        }

        Ok(editor.write()?.detach())
    }

    fn base_tree(&self, entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error> {
        let Some(base) = entry.base else {
            return Ok(None);
        };
        self.upstream_tree(entry, base).map(Some)
    }

    fn ours_tree(
        &self,
        _entry: &VendorEntry,
        _ours: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        todo!()
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
