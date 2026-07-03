use std::io::Write as _;
use std::path::{Path, PathBuf};

use git_vendor::{
    PatternMapping, VendorConfig, VendorEntry, VendorMode, VendorName, VendorRepository,
    VendorStatus, VendorWorktree,
};

use crate::cli;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

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

#[derive(Debug)]
pub struct ConflictExit;

impl std::fmt::Display for ConflictExit {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl std::error::Error for ConflictExit {}

pub struct Executor(pub gix::Repository);

impl Executor {
    pub fn discover() -> Result<Self> {
        Ok(Self(gix::discover(".")?))
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
            None => name_from_url(&url).ok_or_else(|| {
                format!("cannot derive a vendor name from URL {url:?}; pass a name explicitly")
            })?,
        };
        let vendor_name = VendorName::new(&name)?;
        if config.get(vendor_name.as_str())?.is_some() {
            return Err(format!(
                "vendor {name:?} already exists; use `git vendor update {name}` or remove it first"
            )
            .into());
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
                let new_paths = tree_paths(repo, merge.result_tree)?;
                let path_refs: Vec<&gix::bstr::BStr> =
                    new_paths.iter().map(|b| b.as_ref()).collect();
                git_vendor::validate_trackable_paths(&path_refs)?;

                if merge.has_conflicts() {
                    repo.checkout_vendor_conflicted(&entry, &merge)?;
                    reconcile_tracked_paths(repo, &entry, &[], &new_paths, io)?;
                    entry.base = Some(merge.upstream_commit);
                    config.insert(&entry)?;
                    let config_str = save_config(&config, &cfg_path)?;
                    stage_gitvendors(repo, config_str.as_bytes())?;
                    repo.prepare_merge(&entry, &merge, &msg)?;
                    let paths: Vec<_> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
                    writeln!(io.err, "Conflict in: {}", paths.join(", "))?;
                    writeln!(io.err, "Resolve conflicts, then run `git commit`.")?;
                    return Err(ConflictExit.into());
                }

                let _full_tree = repo.checkout_vendor(&entry, merge.result_tree)?;
                reconcile_tracked_paths(repo, &entry, &[], &new_paths, io)?;

                entry.base = Some(merge.upstream_commit);
                config.insert(&entry)?;
                let config_str = save_config(&config, &cfg_path)?;

                stage_gitvendors(repo, config_str.as_bytes())?;
                repo.prepare_merge(&entry, &merge, &msg)?;
                writeln!(io.err, "Staged; run `git commit` to complete.")?;
            }
            None => {
                let tree = repo.upstream_tree(&entry, upstream)?;
                let new_paths = tree_paths(repo, tree)?;
                let path_refs: Vec<&gix::bstr::BStr> =
                    new_paths.iter().map(|b| b.as_ref()).collect();
                git_vendor::validate_trackable_paths(&path_refs)?;

                let _full_tree = repo.checkout_vendor(&entry, tree)?;
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

        let entries: Vec<VendorEntry> = match name {
            Some(ref n) => vec![require_entry(&config, n)?],
            None => config.entries()?,
        };

        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        let head_oid = repo
            .head_commit()
            .map(|c| c.id().detach())
            .map_err(|e| format!("HEAD: {e}"))?;

        let current_head = head_oid;

        for mut entry in entries {
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

            let old_paths: Vec<gix::bstr::BString> = repo.vendor_paths(&entry, current_head)?;

            let merge = repo.merge_vendor(&entry, current_head, upstream)?;
            let new_paths = tree_paths(repo, merge.result_tree)?;
            let path_refs: Vec<&gix::bstr::BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
            git_vendor::validate_trackable_paths(&path_refs)?;

            if merge.has_conflicts() {
                repo.checkout_vendor_conflicted(&entry, &merge)?;
                reconcile_tracked_paths(repo, &entry, &old_paths, &new_paths, io)?;
                entry.base = Some(merge.upstream_commit);
                config.insert(&entry)?;
                let config_str = save_config(&config, &cfg_path)?;
                stage_gitvendors(repo, config_str.as_bytes())?;
                repo.prepare_merge(&entry, &merge, &msg)?;
                let paths: Vec<_> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
                writeln!(io.err, "{n}: conflict in {}", paths.join(", "))?;
                writeln!(io.err, "Resolve conflicts, then run `git commit`.")?;
                return Err(ConflictExit.into());
            }

            let _full_tree = repo.checkout_vendor(&entry, merge.result_tree)?;
            reconcile_tracked_paths(repo, &entry, &old_paths, &new_paths, io)?
                .ok_or("`.gitattributes` has an unresolved conflict; resolve it before updating")?;

            entry.base = Some(merge.upstream_commit);
            config.insert(&entry)?;
            let config_str = save_config(&config, &cfg_path)?;

            stage_gitvendors(repo, config_str.as_bytes())?;
            repo.prepare_merge(&entry, &merge, &msg)?;
            writeln!(io.err, "Updated {n}. Run `git commit` to record the merge.")?;

            // `prepare_merge` overwrites MERGE_HEAD rather than accumulating an
            // octopus merge, so a second vendor's pending merge in the same run
            // would silently clobber this one's. Stop here; re-running `update`
            // after the commit picks up the rest.
            break;
        }

        Ok(())
    }

    /// Rebuild vendored files from `.gitvendors` without fetching (`update
    /// --no-fetch`). Use after editing a vendor's `pattern` entries to move
    /// or refilter its files. Refuses a modified vendor unless `force`.
    fn update_no_fetch(&self, name: Option<String>, force: bool, io: &mut Io) -> Result<()> {
        let repo = &self.0;
        let cfg_path = config_path(repo)?;
        let config = load_config(&cfg_path)?;

        let entries: Vec<VendorEntry> = match name {
            Some(ref n) => vec![require_entry(&config, n)?],
            None => config.entries()?,
        };

        if entries.is_empty() {
            writeln!(io.err, "No vendors configured.")?;
            return Ok(());
        }

        let head_oid = repo
            .head_commit()
            .map(|c| c.id().detach())
            .map_err(|e| format!("HEAD: {e}"))?;

        // Patterns as last committed, for the local-modification check: a vendor
        // whose ours tree differs from the pristine upstream tree of its recorded
        // base carries patches that re-materializing would discard.
        let old_config = config_at(repo, head_oid)?;

        let config_str = save_config(&config, &cfg_path)?;

        for entry in entries {
            let n = entry.name.as_str().to_owned();
            let Some(base) = entry.base else {
                writeln!(
                    io.err,
                    "{n}: no recorded base; run `git vendor update {n}` first"
                )?;
                continue;
            };

            let pristine = old_config
                .as_ref()
                .and_then(|c| c.get(&n).ok().flatten())
                .and_then(|old| old.base.map(|b| (old, b)))
                .map(|(old, b)| repo.upstream_tree(&old, b))
                .transpose()?;
            if let Some(pristine) = pristine {
                let ours = repo.ours_tree(&entry, head_oid)?;
                if ours != pristine && !force {
                    let pristine_blobs = tree_blobs(repo, pristine)?;
                    let our_blobs = tree_blobs(repo, ours)?;
                    let mut modified: Vec<String> = our_blobs
                        .iter()
                        .filter(|(p, oid)| pristine_blobs.get(*p) != Some(oid))
                        .map(|(p, _)| p.to_string())
                        .collect();
                    modified.extend(
                        pristine_blobs
                            .keys()
                            .filter(|p| !our_blobs.contains_key(*p))
                            .map(|p| p.to_string()),
                    );
                    modified.sort();
                    writeln!(
                        io.err,
                        "{n}: vendored files have local modifications ({}); \
                         re-run with --force to discard them",
                        modified.join(", ")
                    )?;
                    continue;
                }
            }

            let new_tree = repo.upstream_tree(&entry, base)?;
            let old_paths: Vec<gix::bstr::BString> = repo.vendor_paths(&entry, head_oid)?;
            let new_paths = tree_paths(repo, new_tree)?;
            let path_refs: Vec<&gix::bstr::BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
            git_vendor::validate_trackable_paths(&path_refs)?;

            let _full_tree = repo.checkout_vendor(&entry, new_tree)?;
            reconcile_tracked_paths(repo, &entry, &old_paths, &new_paths, io)?
                .ok_or("`.gitattributes` has an unresolved conflict; resolve it before updating")?;
            stage_gitvendors(repo, config_str.as_bytes())?;

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

        let entries: Vec<VendorEntry> = match name {
            Some(ref n) => vec![require_entry(&config, n)?],
            None => config.entries()?,
        };

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

        let head_oid = repo.head_commit().ok().map(|c| c.id().detach());
        let paths = match head_oid {
            Some(oid) => repo.vendor_paths(&entry, oid)?,
            // No commit yet: resolve from the staged index instead, since
            // there is no tree to read paths from (e.g. right after `add`).
            None => git_vendor::resolve_vendor_paths_uncommitted(repo, &entry)?,
        };

        {
            use gix::bstr::ByteSlice as _;
            let path_refs: Vec<&gix::bstr::BStr> = paths.iter().map(|b| b.as_ref()).collect();

            if !keep_files {
                let workdir = repo.workdir().ok_or("not a working-copy repository")?;
                for p in &paths {
                    let abs = workdir.join(gix::path::from_bstr(p).as_ref());
                    if abs.symlink_metadata().is_ok() {
                        std::fs::remove_file(&abs)?;
                    }
                }
            }

            repo.untrack_vendor(&entry, &path_refs)?;

            if !keep_files {
                let mut index = repo.open_index().map_err(|e| format!("{e}"))?;
                for p in &path_refs {
                    let pb = p.as_bytes();
                    index.remove_entries(|_, path, _| path == pb.as_bstr());
                }
                index.sort_entries();
                index
                    .write(gix::index::write::Options::default())
                    .map_err(|e| format!("{e}"))?;
            }
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
    let workdir = repo.workdir().ok_or("not a working-copy repository")?;
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
        .ok_or_else(|| format!("no vendor named {name:?}").into())
}

fn tree_paths(repo: &gix::Repository, tree_id: gix::ObjectId) -> Result<Vec<gix::bstr::BString>> {
    let index = repo.index_from_tree(&tree_id)?;
    Ok(index
        .entries()
        .iter()
        .map(|e| e.path(&index).into())
        .collect())
}

fn tree_blobs(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
) -> Result<std::collections::BTreeMap<gix::bstr::BString, gix::ObjectId>> {
    let index = repo.index_from_tree(&tree_id)?;
    Ok(index
        .entries()
        .iter()
        .map(|e| (e.path(&index).into(), e.id))
        .collect())
}

fn config_at(repo: &gix::Repository, commit: gix::ObjectId) -> Result<Option<VendorConfig>> {
    let tree = repo
        .find_commit(commit)
        .map_err(|e| format!("{e}"))?
        .tree()
        .map_err(|e| format!("{e}"))?;
    let Some(entry) = tree
        .lookup_entry_by_path(".gitvendors")
        .map_err(|e| format!("{e}"))?
    else {
        return Ok(None);
    };
    let blob = entry.object().map_err(|e| format!("{e}"))?;
    let s = String::from_utf8_lossy(&blob.data).into_owned();
    Ok(Some(VendorConfig::parse(&s)?))
}

/// Whether `path` currently has any unmerged (non-zero-stage) entry in the
/// index — i.e. it is itself part of an unresolved conflict.
fn has_unmerged_stages(repo: &gix::Repository, path: &gix::bstr::BStr) -> Result<bool> {
    let index = repo.open_index().map_err(|e| format!("{e}"))?;
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

/// Write `content` as a blob, upsert the `.gitvendors` index entry, and return
/// the blob OID so callers can include it in a commit tree.
fn stage_gitvendors(repo: &gix::Repository, content: &[u8]) -> Result<gix::ObjectId> {
    use gix::bstr::ByteSlice as _;
    let blob_oid = repo
        .write_object(gix::objs::BlobRef { data: content })
        .map_err(|e| format!("{e}"))?
        .detach();
    let mut index = repo.open_index().map_err(|e| format!("{e}"))?;
    index.remove_entries(|_, path, _| path == b".gitvendors".as_bstr());
    index.dangerously_push_entry(
        gix::index::entry::Stat::default(),
        blob_oid,
        gix::index::entry::Flags::empty(),
        gix::index::entry::Mode::FILE,
        b".gitvendors".as_bstr(),
    );
    index.sort_entries();
    index
        .write(gix::index::write::Options::default())
        .map_err(|e| format!("{e}"))?;
    Ok(blob_oid)
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
