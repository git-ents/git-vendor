use std::io::Write as _;
use std::path::{Path, PathBuf};

use git_vendor::{
    PatternMapping, VendorConfig, VendorEntry, VendorMode, VendorName, VendorRepository,
    VendorStatus, VendorWorktree,
};

use crate::cli;

type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors surfaced by the `git-vendor` executor.
///
/// Each application-level failure gets its own variant so `main` (and any other
/// caller) can match on it rather than string-sniff — in particular
/// [`Error::Conflict`], which signals that the working tree has been staged
/// with conflict markers and the process should exit non-zero *without*
/// printing a further message. Errors from the vendoring library and from
/// filesystem I/O are wrapped transparently; lower-level `gix` plumbing errors
/// are boxed into [`Error::Gix`], mirroring [`git_vendor::Error`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The current directory is not inside a git repository.
    #[error(transparent)]
    Discover(#[from] Box<gix::discover::Error>),

    /// The repository has no working copy (it is bare).
    #[error("not a working-copy repository")]
    NotWorkingCopy,

    /// A vendor with the requested name already exists.
    #[error("vendor {0:?} already exists; use `git vendor update {0}` or remove it first")]
    VendorExists(String),

    /// No vendor with the requested name is configured.
    #[error("no vendor named {0:?}")]
    NoSuchVendor(String),

    /// No name was given and none could be derived from the URL.
    #[error("cannot derive a vendor name from URL {0:?}; pass a name explicitly")]
    UndeterminableName(String),

    /// `.gitattributes` carries an unresolved conflict that blocks the update.
    #[error("`.gitattributes` has an unresolved conflict; resolve it before updating")]
    GitattributesConflict,

    /// Reading `HEAD` failed.
    #[error("HEAD: {0}")]
    Head(#[source] Box<gix::reference::head_commit::Error>),

    /// The merge left unresolved conflicts. The working tree has already been
    /// staged and actionable guidance printed to stderr; `main` exits non-zero
    /// without rendering this error.
    #[error("unresolved merge conflict")]
    Conflict,

    /// An error from the vendoring library.
    #[error(transparent)]
    Vendor(#[from] git_vendor::Error),

    /// A filesystem I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A lower-level `gix` error with no more specific variant.
    #[error(transparent)]
    Gix(Box<dyn std::error::Error + Send + Sync + 'static>),
}

macro_rules! impl_gix_from {
    ($($ty:path),* $(,)?) => {
        $(
            impl From<$ty> for Error {
                fn from(e: $ty) -> Self {
                    Error::Gix(Box::new(e))
                }
            }
        )*
    };
}

impl_gix_from! {
    gix::repository::index_from_tree::Error,
    gix::object::find::existing::with_conversion::Error,
    gix::object::commit::Error,
    gix::worktree::open_index::Error,
    gix::reference::find::Error,
    gix::reference::edit::Error,
    gix::index::file::write::Error,
}

pub struct Io {
    pub out: Box<dyn std::io::Write>,
    pub err: Box<dyn std::io::Write>,
}

impl Io {
    pub fn stdio() -> Self {
        Io {
            out: Box::new(std::io::stdout()),
            err: Box::new(std::io::stderr()),
        }
    }
}

pub struct Executor(pub gix::Repository);

impl Executor {
    pub fn discover() -> Result<Self> {
        Ok(Self(gix::discover(".").map_err(Box::new)?))
    }

    pub fn run(&self, cli: cli::Cli, io: &mut Io) -> Result<()> {
        match cli.command {
            cli::Command::Add {
                url,
                name,
                ref_name,
                prefix,
                patterns,
                squash,
                dry_run,
                message,
            } => self.add(
                name, url, ref_name, prefix, patterns, squash, dry_run, message, io,
            ),
            cli::Command::Update {
                name,
                message,
                force,
                no_fetch,
                dry_run,
            } => self.update(name, message, force, no_fetch, dry_run, io),
            cli::Command::Status { name, fetch } => self.status(name, fetch, io),
            cli::Command::Remove { name, keep_files } => self.remove(name, keep_files, io),
            cli::Command::List => self.list(io),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &self,
        name: Option<String>,
        url: String,
        ref_name: Option<String>,
        prefix: Option<String>,
        patterns: Vec<String>,
        squash: bool,
        dry_run: bool,
        message: Option<String>,
        io: &mut Io,
    ) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let mut config = load_config(&cfg_path)?;

        let name = match name {
            Some(n) => n,
            None => name_from_url(&url).ok_or_else(|| Error::UndeterminableName(url.clone()))?,
        };
        let vendor_name = VendorName::new(&name)?;
        if config.get(vendor_name.as_str())?.is_some() {
            return Err(Error::VendorExists(name.clone()));
        }
        let mode = if squash {
            VendorMode::Squash
        } else {
            VendorMode::default()
        };
        let patterns: Vec<String> = if patterns.is_empty() {
            let dest = prefix.map_or_else(
                || format!("vendor/{name}/"),
                |p| {
                    if p.ends_with('/') { p } else { format!("{p}/") }
                },
            );
            vec![format!("**:{dest}")]
        } else {
            patterns
        };
        let mut entry = VendorEntry {
            name: vendor_name,
            url,
            ref_name,
            base: None,
            patterns: patterns.iter().map(|p| PatternMapping::parse(p)).collect(),
            mode,
        };

        // Fetch before touching config — a failed fetch leaves no side effects.
        writeln!(io.err, "Fetching {name}…")?;
        let upstream = repo.fetch_vendor(&entry)?;

        if dry_run {
            writeln!(io.err, "Would add vendor {name} at {upstream}.")?;
            return Ok(());
        }

        let head_oid = repo.head_commit().ok().map(|c| c.id().detach());

        let msg = message
            .clone()
            .unwrap_or_else(|| format!("vendor: add {name}"));

        match head_oid {
            Some(ours) => {
                let merge = repo.merge_vendor(&entry, ours, upstream)?;
                let conflicted = self.apply_merge(
                    &mut config,
                    &cfg_path,
                    &mut entry,
                    &merge,
                    &[],
                    &msg,
                    false,
                    io,
                )?;
                if conflicted {
                    return Err(Error::Conflict);
                }
                writeln!(io.err, "Staged; run `git commit` to complete.")?;
            }
            None => {
                // An unborn HEAD has no merge to seal, so the tool writes no
                // MERGE_MSG and never commits: a `-m` message has nowhere to go.
                if message.is_some() {
                    writeln!(
                        io.err,
                        "note: --message is ignored on an unborn HEAD; pass it to your own \
                         `git commit`."
                    )?;
                }
                let tree = repo.upstream_tree(&entry, upstream)?;
                let new_paths = tree_paths(repo, tree)?;
                let path_refs: Vec<&gix::bstr::BStr> =
                    new_paths.iter().map(|b| b.as_ref()).collect();
                git_vendor::validate_trackable_paths(&path_refs)?;

                repo.checkout_vendor(&entry, tree)?;
                repo.track_vendor(&entry, &path_refs)?;

                entry.base = Some(upstream);
                config.insert(&entry)?;
                let config_str = save_config(&config, &cfg_path)?;

                stage_gitvendors(repo, config_str.as_bytes())?;
                writeln!(io.err, "Staged; run `git commit` to complete.")?;
            }
        }

        Ok(())
    }

    fn update(
        &self,
        name: Option<String>,
        message: Option<String>,
        force: bool,
        no_fetch: bool,
        dry_run: bool,
        io: &mut Io,
    ) -> Result<()> {
        if no_fetch {
            return self.update_no_fetch(name, force, io);
        }

        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let mut config = load_config(&cfg_path)?;

        let entries = resolve_entries(&config, name.as_deref())?;
        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        let head_oid = repo
            .head_commit()
            .map(|c| c.id().detach())
            .map_err(|e| Error::Head(Box::new(e)))?;

        let total = entries.len();
        for (i, mut entry) in entries.into_iter().enumerate() {
            let n = entry.name.as_str().to_owned();
            writeln!(io.err, "Fetching {n}…")?;
            let upstream = repo.fetch_vendor(&entry)?;

            let status = repo.vendor_status(&entry)?;
            match status {
                VendorStatus::UpToDate => {
                    writeln!(io.err, "{n}: already up to date")?;
                    continue;
                }
                VendorStatus::ForcePushed { .. } if !force => {
                    writeln!(
                        io.err,
                        "{n}: upstream was force-pushed; re-run with --force to accept"
                    )?;
                    continue;
                }
                _ => {}
            }

            if dry_run {
                writeln!(io.err, "Would update {n} to {upstream}.")?;
                continue;
            }

            let msg = message
                .clone()
                .unwrap_or_else(|| format!("vendor: update {n}"));

            let old_paths: Vec<gix::bstr::BString> = repo.vendor_paths(&entry, head_oid)?;
            let merge = repo.merge_vendor(&entry, head_oid, upstream)?;
            let conflicted = self.apply_merge(
                &mut config,
                &cfg_path,
                &mut entry,
                &merge,
                &old_paths,
                &msg,
                true,
                io,
            )?;
            if conflicted {
                return Err(Error::Conflict);
            }
            writeln!(io.err, "Updated {n}. Run `git commit` to record the merge.")?;

            // `prepare_merge` overwrites MERGE_HEAD rather than accumulating an
            // octopus merge, so a second vendor's pending merge in the same run
            // would silently clobber this one's. Stop here; re-running `update`
            // after the commit picks up the rest.
            let remaining = total - i - 1;
            if remaining > 0 {
                writeln!(
                    io.err,
                    "Stopped after {n}; {remaining} more vendor(s) not yet processed — \
                     re-run `git vendor update` after committing this merge."
                )?;
            }
            break;
        }

        Ok(())
    }

    /// Validate and stage a merge result shared by `add` and `update`: checks
    /// that upstream paths are trackable, checks out the merged tree
    /// (conflicted or clean), reconciles `.gitattributes`, records the new
    /// base, and stages `.gitvendors`. Returns `Ok(true)` if the merge left
    /// conflicts, having already printed the conflict message — the caller
    /// should return `Err(Error::Conflict)`. `require_reconcile` controls
    /// whether an unresolved `.gitattributes` conflict on a clean merge is
    /// itself treated as an error (true for `update`, which has old paths to
    /// reconcile against; false for `add`'s first-merge case, which has none).
    #[allow(clippy::too_many_arguments)]
    fn apply_merge(
        &self,
        config: &mut VendorConfig,
        cfg_path: &Path,
        entry: &mut VendorEntry,
        merge: &git_vendor::VendorMerge,
        old_paths: &[gix::bstr::BString],
        msg: &str,
        require_reconcile: bool,
        io: &mut Io,
    ) -> Result<bool> {
        let repo = &self.0;
        let new_paths = tree_paths(repo, merge.result_tree)?;
        let path_refs: Vec<&gix::bstr::BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
        git_vendor::validate_trackable_paths(&path_refs)?;

        if merge.has_conflicts() {
            repo.checkout_vendor_conflicted(entry, merge)?;
            reconcile_tracked_paths(repo, entry, old_paths, &new_paths, io)?;
            entry.base = Some(merge.upstream_commit);
            config.insert(entry)?;
            let config_str = save_config(config, cfg_path)?;
            stage_gitvendors(repo, config_str.as_bytes())?;
            repo.prepare_merge(entry, merge, msg)?;
            let paths: Vec<_> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
            writeln!(io.err, "{}: conflict in {}", entry.name, paths.join(", "))?;
            writeln!(io.err, "Resolve conflicts, then run `git commit`.")?;
            return Ok(true);
        }

        repo.checkout_vendor(entry, merge.result_tree)?;
        let reconciled = reconcile_tracked_paths(repo, entry, old_paths, &new_paths, io)?;
        if require_reconcile {
            reconciled
                .ok_or(Error::GitattributesConflict)?;
        }

        entry.base = Some(merge.upstream_commit);
        config.insert(entry)?;
        let config_str = save_config(config, cfg_path)?;

        stage_gitvendors(repo, config_str.as_bytes())?;
        repo.prepare_merge(entry, merge, msg)?;
        Ok(false)
    }

    /// Rebuild vendored files from `.gitvendors` without fetching (`update
    /// --no-fetch`). Use after editing a vendor's `pattern` entries to move
    /// or refilter its files. Refuses a modified vendor unless `force`.
    fn update_no_fetch(&self, name: Option<String>, force: bool, io: &mut Io) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let config = load_config(&cfg_path)?;

        let entries = resolve_entries(&config, name.as_deref())?;
        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        let head_oid = repo
            .head_commit()
            .map(|c| c.id().detach())
            .map_err(|e| Error::Head(Box::new(e)))?;

        // Patterns as last committed, for the local-modification check: a vendor
        // whose ours tree differs from the pristine upstream tree of its recorded
        // base carries patches that re-materializing would discard.
        let old_config = config_at(repo, head_oid)?;

        // Stage the user's on-disk `.gitvendors` verbatim, and only when a
        // vendor is actually rebuilt. Re-serializing it here would normalize
        // the file's formatting even on a run where every vendor is skipped.
        let cfg_bytes = std::fs::read(&cfg_path).unwrap_or_default();

        for entry in entries {
            let n = entry.name.as_str().to_owned();
            let Some(base) = entry.base else {
                writeln!(
                    io.err,
                    "{n}: no recorded base; run `git vendor update {n}` first"
                )?;
                continue;
            };

            let modified = locally_modified_paths(repo, old_config.as_ref(), &entry, head_oid)?;
            if !modified.is_empty() && !force {
                writeln!(
                    io.err,
                    "{n}: vendored files have local modifications ({}); \
                     re-run with --force to discard them",
                    modified.join(", ")
                )?;
                continue;
            }

            let new_tree = repo.upstream_tree(&entry, base)?;
            let old_paths: Vec<gix::bstr::BString> = repo.vendor_paths(&entry, head_oid)?;
            let new_paths = tree_paths(repo, new_tree)?;
            let path_refs: Vec<&gix::bstr::BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
            git_vendor::validate_trackable_paths(&path_refs)?;

            repo.checkout_vendor(&entry, new_tree)?;
            reconcile_tracked_paths(repo, &entry, &old_paths, &new_paths, io)?
                .ok_or(Error::GitattributesConflict)?;
            stage_gitvendors(repo, &cfg_bytes)?;

            writeln!(
                io.err,
                "Updated {n}. Run `git commit` to record the change."
            )?;
        }

        Ok(())
    }

    fn status(&self, name: Option<String>, fetch: bool, io: &mut Io) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let config = load_config(&cfg_path)?;

        let entries = resolve_entries(&config, name.as_deref())?;
        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        for entry in &entries {
            if fetch {
                repo.fetch_vendor(entry)?;
            }
            let status = repo.vendor_status(entry)?;
            let label = match &status {
                VendorStatus::NotFetched => "not fetched".to_owned(),
                VendorStatus::UpToDate => "up to date".to_owned(),
                VendorStatus::UpdateAvailable { upstream } => {
                    format!("update available ({})", upstream.to_hex())
                }
                VendorStatus::ForcePushed { upstream } => {
                    format!("force-pushed upstream ({})", upstream.to_hex())
                }
            };
            writeln!(io.out, "{}\t{}\t{label}", entry.name, entry.url)?;
        }

        Ok(())
    }

    fn remove(&self, name: String, keep_files: bool, io: &mut Io) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let mut config = load_config(&cfg_path)?;

        let entry = require_entry(&config, &name)?;

        // Resolve the vendor's paths from HEAD *and* the staged index, unioned.
        // HEAD alone misses a vendor added but not yet committed (its tracking
        // lives only in the staged `.gitattributes`, not any committed tree),
        // which would leave its files on disk, in the index, and in
        // `.gitattributes` while reporting success.
        let head_oid = repo.head_commit().ok().map(|c| c.id().detach());
        let mut path_set: std::collections::BTreeSet<gix::bstr::BString> =
            std::collections::BTreeSet::new();
        if let Some(oid) = head_oid {
            path_set.extend(repo.vendor_paths(&entry, oid)?);
        }
        path_set.extend(git_vendor::resolve_vendor_paths_uncommitted(repo, &entry)?);
        let paths: Vec<gix::bstr::BString> = path_set.into_iter().collect();

        {
            use gix::bstr::ByteSlice as _;
            let path_refs: Vec<&gix::bstr::BStr> = paths.iter().map(|b| b.as_ref()).collect();

            if !keep_files {
                let workdir = repo.workdir().ok_or(Error::NotWorkingCopy)?;
                for p in &paths {
                    let abs = workdir.join(gix::path::from_bstr(p).as_ref());
                    if abs.symlink_metadata().is_ok() {
                        std::fs::remove_file(&abs)?;
                    }
                    remove_empty_ancestors(&abs, workdir);
                }
            }

            repo.untrack_vendor(&entry, &path_refs)?;

            if !keep_files {
                let mut index = repo.open_index()?;
                for p in &path_refs {
                    let pb = p.as_bytes();
                    index.remove_entries(|_, path, _| path == pb.as_bstr());
                }
                index.sort_entries();
                index.write(gix::index::write::Options::default())?;
            }
        }

        // Drop the private `refs/vendor/<name>` tracking ref so a removed
        // vendor doesn't leak a ref (and a re-add starts from a clean slate).
        if let Some(reference) = repo.try_find_reference(&entry.vendor_ref())? {
            reference.delete()?;
        }

        config.remove(&name)?;
        let config_str = save_config(&config, &cfg_path)?;
        stage_gitvendors(repo, config_str.as_bytes())?;
        writeln!(io.err, "Removed vendor {name}.")?;
        Ok(())
    }

    fn list(&self, io: &mut Io) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let config = load_config(&cfg_path)?;
        let entries = config.entries()?;

        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        for entry in &entries {
            let ref_label = entry.ref_name.as_deref().unwrap_or("HEAD");
            let mode_label = entry.mode.as_str();
            writeln!(
                io.out,
                "{}\t{}\t{ref_label}\t{mode_label}",
                entry.name, entry.url
            )?;
        }

        Ok(())
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn config_path(repo: &gix::Repository) -> Result<PathBuf> {
    let workdir = repo.workdir().ok_or(Error::NotWorkingCopy)?;
    Ok(workdir.join(".gitvendors"))
}

fn load_config(path: &Path) -> Result<VendorConfig> {
    if path.exists() {
        Ok(VendorConfig::open(path)?)
    } else {
        Ok(VendorConfig::parse("")?)
    }
}

/// Write `config` to `path` and return the serialized bytes for blob staging.
fn save_config(config: &VendorConfig, path: &Path) -> Result<String> {
    let s = config.to_string();
    std::fs::write(path, &s)?;
    Ok(s)
}

fn require_entry(config: &VendorConfig, name: &str) -> Result<VendorEntry> {
    config
        .get(name)?
        .ok_or_else(|| Error::NoSuchVendor(name.to_owned()))
}

/// Remove `path`'s parent directory and each ancestor above it, as long as
/// they're empty and still inside `workdir`. Stops at the first non-empty or
/// out-of-bounds directory.
fn remove_empty_ancestors(path: &Path, workdir: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == workdir || !d.starts_with(workdir) {
            break;
        }
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Resolve `name` to a single-entry list, or all configured vendors if omitted.
fn resolve_entries(config: &VendorConfig, name: Option<&str>) -> Result<Vec<VendorEntry>> {
    match name {
        Some(n) => Ok(vec![require_entry(config, n)?]),
        None => Ok(config.entries()?),
    }
}

/// Local-modification guard for `update --no-fetch`: compares `entry`'s
/// current working tree against the pristine upstream tree of its last
/// recorded base, returning the sorted list of differing paths (empty if
/// unmodified or if there's no prior recorded base to compare against).
fn locally_modified_paths(
    repo: &gix::Repository,
    old_config: Option<&VendorConfig>,
    entry: &VendorEntry,
    head_oid: gix::ObjectId,
) -> Result<Vec<String>> {
    let pristine = old_config
        .and_then(|c| c.get(entry.name.as_str()).ok().flatten())
        .and_then(|old| old.base.map(|b| (old, b)))
        .map(|(old, b)| repo.upstream_tree(&old, b))
        .transpose()?;
    let Some(pristine) = pristine else {
        return Ok(Vec::new());
    };

    let ours = repo.ours_tree(entry, head_oid)?;
    if ours == pristine {
        return Ok(Vec::new());
    }

    let pristine_blobs = tree_blobs(repo, pristine)?;
    let our_blobs = tree_blobs(repo, ours)?;
    let mut modified: Vec<String> = our_blobs
        .iter()
        .filter(|(p, entry)| pristine_blobs.get(*p) != Some(entry))
        .map(|(p, _)| p.to_string())
        .collect();
    modified.extend(
        pristine_blobs
            .keys()
            .filter(|p| !our_blobs.contains_key(*p))
            .map(|p| p.to_string()),
    );
    modified.sort();
    Ok(modified)
}

fn tree_paths(repo: &gix::Repository, tree_id: gix::ObjectId) -> Result<Vec<gix::bstr::BString>> {
    let index = repo.index_from_tree(&tree_id)?;
    Ok(index
        .entries()
        .iter()
        .map(|e| e.path(&index).into())
        .collect())
}

/// Map each blob path in `tree_id` to its `(oid, mode)`. The mode is included
/// so an executable-bit-only change (same content, different mode) still reads
/// as a local modification.
fn tree_blobs(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
) -> Result<std::collections::BTreeMap<gix::bstr::BString, (gix::ObjectId, gix::index::entry::Mode)>>
{
    let index = repo.index_from_tree(&tree_id)?;
    Ok(index
        .entries()
        .iter()
        .map(|e| (e.path(&index).into(), (e.id, e.mode)))
        .collect())
}

fn config_at(repo: &gix::Repository, commit: gix::ObjectId) -> Result<Option<VendorConfig>> {
    let tree = repo.find_commit(commit)?.tree()?;
    let Some(entry) = tree
        .lookup_entry_by_path(".gitvendors")
        .map_err(|e| Error::Gix(Box::new(e)))?
    else {
        return Ok(None);
    };
    let blob = entry.object().map_err(|e| Error::Gix(Box::new(e)))?;
    // Error on invalid UTF-8 rather than lossily mangling a `.gitvendors` whose
    // bytes we'd otherwise silently corrupt.
    Ok(Some(VendorConfig::open_from_bytes(&blob.data)?))
}

/// Whether `path` currently has any unmerged (non-zero-stage) entry in the
/// index — i.e. it is itself part of an unresolved conflict.
fn has_unmerged_stages(repo: &gix::Repository, path: &gix::bstr::BStr) -> Result<bool> {
    let index = repo.open_index()?;
    Ok(index.entries().iter().any(|e| {
        e.path(&index) == path && e.flags.stage() != gix::index::entry::Stage::Unconflicted
    }))
}

/// Update `.gitattributes` tracking for a vendor's path set.
///
/// A vendor whose destination pattern maps onto `.gitattributes` itself can
/// leave that path with unmerged stages after `checkout_vendor_conflicted`
/// spliced in a genuine merge conflict on it. Writing new tracking lines in
/// that case would silently collapse the conflict to a single resolved
/// stage-0 entry, so `git commit` would succeed despite the unresolved
/// conflict. When that happens, this leaves the unmerged stages untouched and
/// returns `None` instead.
fn reconcile_tracked_paths(
    repo: &gix::Repository,
    entry: &VendorEntry,
    old_paths: &[gix::bstr::BString],
    new_paths: &[gix::bstr::BString],
    io: &mut Io,
) -> Result<Option<gix::ObjectId>> {
    use gix::bstr::{BStr, ByteSlice as _};

    if has_unmerged_stages(repo, b".gitattributes".as_bstr())? {
        writeln!(
            io.err,
            "{}: .gitattributes itself is part of this conflict; resolve it \
             manually, including the vendor={} tracking lines, before committing",
            entry.name, entry.name,
        )?;
        return Ok(None);
    }

    let track: Vec<&BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
    let attrs_oid = repo.track_vendor(entry, &track)?;

    let new_set: std::collections::HashSet<&[u8]> =
        new_paths.iter().map(|b| b.as_slice()).collect();
    let removed: Vec<&BStr> = old_paths
        .iter()
        .filter(|b| !new_set.contains(b.as_slice()))
        .map(|b| b.as_ref())
        .collect();
    if !removed.is_empty()
        && let Some(oid) = repo.untrack_vendor(entry, &removed)?
    {
        return Ok(Some(oid));
    }
    Ok(Some(attrs_oid))
}

/// Write `content` as a blob and upsert the `.gitvendors` index entry to point
/// at it, returning the blob OID.
fn stage_gitvendors(repo: &gix::Repository, content: &[u8]) -> Result<gix::ObjectId> {
    use gix::bstr::ByteSlice as _;
    Ok(git_vendor::stage_file(
        repo,
        b".gitvendors".as_bstr(),
        content,
    )?)
}

fn name_from_url(url: &str) -> Option<String> {
    let stem = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .find(|s| !s.is_empty())?;
    let stem = stem
        .strip_suffix(".git")
        .or_else(|| stem.strip_suffix(".bundle"))
        .unwrap_or(stem);
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_owned())
    }
}

#[cfg(test)]
#[path = "exe_tests.rs"]
mod tests;
