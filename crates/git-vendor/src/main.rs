#![allow(missing_docs)]

mod cli;

use std::path::{Path, PathBuf};

use clap::Parser as _;
use git_vendor::{
    PatternMapping, VendorConfig, VendorEntry, VendorMode, VendorName, VendorRepository,
    VendorStatus, VendorWorktree,
};

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: cli::Cli) -> Result<()> {
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
        } => cmd_add(
            name, url, ref_name, prefix, patterns, squash, dry_run, message,
        ),
        cli::Command::Update {
            name,
            message,
            force,
            dry_run,
        } => cmd_update(name, message, force, dry_run),
        cli::Command::Status { name, fetch } => cmd_status(name, fetch),
        cli::Command::Remove { name, keep_files } => cmd_remove(name, keep_files),
        cli::Command::List => cmd_list(),
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn discover() -> Result<gix::Repository> {
    Ok(gix::discover(".")?)
}

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

fn advance_head(repo: &gix::Repository, new_commit: gix::ObjectId, msg: &str) -> Result<()> {
    use gix::refs::Target;
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

    let name: gix::refs::FullName = "HEAD".try_into()?;
    repo.edit_references([RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: msg.as_bytes().into(),
            },
            expected: PreviousValue::Any,
            new: Target::Object(new_commit),
        },
        name,
        deref: true,
    }])?;
    Ok(())
}

fn committer_sig(repo: &gix::Repository) -> Result<gix::actor::Signature> {
    let sig_ref = repo
        .committer()
        .ok_or("no committer identity; set user.name and user.email")?
        .map_err(|e| format!("committer: {e}"))?;
    sig_ref
        .to_owned()
        .map_err(|e| format!("committer time: {e}").into())
}

fn author_sig(repo: &gix::Repository) -> Result<gix::actor::Signature> {
    let sig_ref = repo
        .author()
        .ok_or("no author identity; set user.name and user.email")?
        .map_err(|e| format!("author: {e}"))?;
    sig_ref
        .to_owned()
        .map_err(|e| format!("author time: {e}").into())
}

fn reconcile_tracked_paths(
    repo: &gix::Repository,
    entry: &VendorEntry,
    old_paths: &[gix::bstr::BString],
    new_paths: &[gix::bstr::BString],
) -> Result<()> {
    use gix::bstr::BStr;
    let track: Vec<&BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
    repo.track_vendor(entry, &track)?;

    let new_set: std::collections::HashSet<&[u8]> =
        new_paths.iter().map(|b| b.as_slice()).collect();
    let removed: Vec<&BStr> = old_paths
        .iter()
        .filter(|b| !new_set.contains(b.as_slice()))
        .map(|b| b.as_ref())
        .collect();
    if !removed.is_empty() {
        repo.untrack_vendor(entry, &removed)?;
    }
    Ok(())
}

/// Write `content` as a blob, upsert the `.gitattributes` index entry, and
/// return the blob OID so callers can include it in a commit tree.
fn stage_attrs_blob(repo: &gix::Repository) -> Result<gix::ObjectId> {
    use gix::bstr::ByteSlice as _;
    let index = repo.open_index().map_err(|e| format!("{e}"))?;
    index
        .entries()
        .iter()
        .find(|e| e.path(&index) == b".gitattributes".as_bstr())
        .map(|e| e.id)
        .ok_or_else(|| "no .gitattributes in index after tracking".into())
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

/// Upsert `.gitattributes` and `.gitvendors` blobs into `full_tree`, returning
/// the corrected tree OID that carries both files.
fn final_tree(
    repo: &gix::Repository,
    full_tree: gix::ObjectId,
    attrs_blob: gix::ObjectId,
    vendors_blob: gix::ObjectId,
) -> Result<gix::ObjectId> {
    use gix::bstr::ByteSlice as _;
    let mut editor = repo
        .find_object(full_tree)
        .map_err(|e| format!("{e}"))?
        .into_tree()
        .edit()
        .map_err(|e| format!("{e}"))?;
    editor
        .upsert(
            b".gitattributes".as_bstr(),
            gix::objs::tree::EntryKind::Blob,
            attrs_blob,
        )
        .map_err(|e| format!("{e}"))?;
    editor
        .upsert(
            b".gitvendors".as_bstr(),
            gix::objs::tree::EntryKind::Blob,
            vendors_blob,
        )
        .map_err(|e| format!("{e}"))?;
    Ok(editor.write().map_err(|e| format!("{e}"))?.detach())
}

/// Mint a vendor merge commit using `tree` and advance HEAD.
///
/// In squash mode a parentless squash commit is minted and used as the
/// second parent; in merge mode the upstream commit is used directly.
fn commit_and_advance(
    repo: &gix::Repository,
    entry: &VendorEntry,
    merge: &git_vendor::VendorMerge,
    tree: gix::ObjectId,
    parent: gix::ObjectId,
    message: &str,
) -> Result<()> {
    let author = author_sig(repo)?;
    let committer = committer_sig(repo)?;

    let mut tbuf_a = gix::date::parse::TimeBuf::default();
    let mut tbuf_c = gix::date::parse::TimeBuf::default();

    let second_parent = if entry.mode == VendorMode::Squash {
        let upstream_tree = repo.upstream_tree(entry, merge.upstream_commit)?;
        let squash = gix::objs::Commit {
            tree: upstream_tree,
            parents: Default::default(),
            author: author.to_ref(&mut tbuf_a).into(),
            committer: committer.to_ref(&mut tbuf_c).into(),
            encoding: None,
            message: format!(
                "squash: vendor '{}'\n\nSquashed-upstream: {}\n",
                entry.name, merge.upstream_commit
            )
            .into(),
            extra_headers: Vec::new(),
        };
        repo.write_object(&squash)?.detach()
    } else {
        merge.upstream_commit
    };

    let mut tbuf_a2 = gix::date::parse::TimeBuf::default();
    let mut tbuf_c2 = gix::date::parse::TimeBuf::default();
    let commit = gix::objs::Commit {
        tree,
        parents: [parent, second_parent].into_iter().collect(),
        author: author.to_ref(&mut tbuf_a2).into(),
        committer: committer.to_ref(&mut tbuf_c2).into(),
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };
    let new_commit = repo.write_object(&commit)?.detach();
    advance_head(repo, new_commit, message)
}

// ── commands ─────────────────────────────────────────────────────────────────

/// Derive a vendor name from a URL by taking the last non-empty path component
/// and stripping common suffixes (`.git`, `.bundle`).
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

#[allow(clippy::too_many_arguments)]
fn cmd_add(
    name: Option<String>,
    url: String,
    ref_name: Option<String>,
    prefix: Option<String>,
    patterns: Vec<String>,
    squash: bool,
    dry_run: bool,
    message: Option<String>,
) -> Result<()> {
    let repo = discover()?;
    let cfg_path = config_path(&repo)?;
    let mut config = load_config(&cfg_path)?;

    let name = match name {
        Some(n) => n,
        None => name_from_url(&url).ok_or_else(|| {
            format!("cannot derive a vendor name from URL {url:?}; pass a name explicitly")
        })?,
    };
    let vendor_name = VendorName::new(&name)?;
    let mode = if squash {
        VendorMode::Squash
    } else {
        VendorMode::default()
    };
    let patterns: Vec<String> = if patterns.is_empty() {
        let dest = prefix.unwrap_or_else(|| format!("vendor/{name}/"));
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
    eprintln!("Fetching {name}…");
    let upstream = repo.fetch_vendor(&entry)?;

    if dry_run {
        eprintln!("Would add vendor {name} at {upstream}.");
        return Ok(());
    }

    let head_oid = repo.head_commit().ok().map(|c| c.id().detach());

    let msg = message
        .clone()
        .unwrap_or_else(|| format!("vendor: add {name}"));

    match head_oid {
        Some(ours) => {
            let merge = repo.merge_vendor(&entry, ours, upstream)?;

            if merge.has_conflicts() {
                repo.checkout_vendor_conflicted(&entry, &merge)?;
                // Record the config entry and set up MERGE_HEAD so the user's
                // `git commit` after resolution produces a proper merge commit.
                entry.base = Some(merge.upstream_commit);
                config.insert(&entry)?;
                let config_str = save_config(&config, &cfg_path)?;
                stage_gitvendors(&repo, config_str.as_bytes())?;
                repo.prepare_merge(&entry, &merge, &msg)?;
                let paths: Vec<_> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
                eprintln!("Conflict in: {}", paths.join(", "));
                eprintln!("Resolve conflicts, then run `git commit`.");
                std::process::exit(1);
            }

            let full_tree = repo.checkout_vendor(&entry, merge.result_tree)?;
            let new_paths = tree_paths(&repo, merge.result_tree)?;
            reconcile_tracked_paths(&repo, &entry, &[], &new_paths)?;

            entry.base = Some(merge.upstream_commit);
            config.insert(&entry)?;
            let config_str = save_config(&config, &cfg_path)?;

            let attrs_blob = stage_attrs_blob(&repo)?;
            let vendors_blob = stage_gitvendors(&repo, config_str.as_bytes())?;
            let tree = final_tree(&repo, full_tree, attrs_blob, vendors_blob)?;
            commit_and_advance(&repo, &entry, &merge, tree, ours, &msg)?;
            eprintln!("Added vendor {name}.");
        }
        None => {
            // Unborn repository: make the initial commit directly (no merge).
            let tree = repo.upstream_tree(&entry, upstream)?;
            let full_tree = repo.checkout_vendor(&entry, tree)?;
            let new_paths = tree_paths(&repo, tree)?;
            let path_refs: Vec<&gix::bstr::BStr> = new_paths.iter().map(|b| b.as_ref()).collect();
            repo.track_vendor(&entry, &path_refs)?;

            entry.base = Some(upstream);
            config.insert(&entry)?;
            let config_str = save_config(&config, &cfg_path)?;

            let attrs_blob = stage_attrs_blob(&repo)?;
            let vendors_blob = stage_gitvendors(&repo, config_str.as_bytes())?;
            let commit_tree = final_tree(&repo, full_tree, attrs_blob, vendors_blob)?;

            let author = author_sig(&repo)?;
            let committer = committer_sig(&repo)?;
            let mut tbuf_a = gix::date::parse::TimeBuf::default();
            let mut tbuf_c = gix::date::parse::TimeBuf::default();
            let commit = gix::objs::Commit {
                tree: commit_tree,
                parents: Default::default(),
                author: author.to_ref(&mut tbuf_a).into(),
                committer: committer.to_ref(&mut tbuf_c).into(),
                encoding: None,
                message: msg.as_str().into(),
                extra_headers: Vec::new(),
            };
            let new_commit = repo.write_object(&commit)?.detach();
            advance_head(&repo, new_commit, &msg)?;
            eprintln!("Added vendor {name}.");
        }
    }

    Ok(())
}

fn cmd_update(
    name: Option<String>,
    message: Option<String>,
    force: bool,
    dry_run: bool,
) -> Result<()> {
    let repo = discover()?;
    let cfg_path = config_path(&repo)?;
    let mut config = load_config(&cfg_path)?;

    // Multi-vendor updates always auto-commit (one commit per vendor); only a
    // single-vendor update without -m uses the prepare-merge path.
    let auto_commit = name.is_none() || message.is_some();

    let entries: Vec<VendorEntry> = match name {
        Some(ref n) => vec![require_entry(&config, n)?],
        None => config.entries()?,
    };

    if entries.is_empty() {
        eprintln!("No vendors configured.");
        return Ok(());
    }

    let head_oid = repo
        .head_commit()
        .map(|c| c.id().detach())
        .map_err(|e| format!("HEAD: {e}"))?;

    let mut current_head = head_oid;

    for mut entry in entries {
        let n = entry.name.as_str().to_owned();
        eprintln!("Fetching {n}…");
        let upstream = repo.fetch_vendor(&entry)?;

        let status = repo.vendor_status(&entry)?;
        match status {
            VendorStatus::UpToDate => {
                eprintln!("{n}: already up to date");
                continue;
            }
            VendorStatus::ForcePushed { .. } if !force => {
                eprintln!("{n}: upstream was force-pushed; re-run with --force to accept");
                continue;
            }
            _ => {}
        }

        if dry_run {
            eprintln!("Would update {n} to {upstream}.");
            continue;
        }

        let msg = message
            .clone()
            .unwrap_or_else(|| format!("vendor: update {n}"));

        let old_paths: Vec<gix::bstr::BString> =
            repo.vendor_paths(&entry, current_head).unwrap_or_default();

        let merge = repo.merge_vendor(&entry, current_head, upstream)?;

        if merge.has_conflicts() {
            repo.checkout_vendor_conflicted(&entry, &merge)?;
            entry.base = Some(merge.upstream_commit);
            config.insert(&entry)?;
            let config_str = save_config(&config, &cfg_path)?;
            stage_gitvendors(&repo, config_str.as_bytes())?;
            repo.prepare_merge(&entry, &merge, &msg)?;
            let paths: Vec<_> = merge.conflicts.iter().map(|c| c.path.as_str()).collect();
            eprintln!("{n}: conflict in {}", paths.join(", "));
            eprintln!("Resolve conflicts, then run `git commit`.");
            std::process::exit(1);
        }

        let full_tree = repo.checkout_vendor(&entry, merge.result_tree)?;
        let new_paths = tree_paths(&repo, merge.result_tree)?;
        reconcile_tracked_paths(&repo, &entry, &old_paths, &new_paths)?;

        entry.base = Some(merge.upstream_commit);
        config.insert(&entry)?;
        let config_str = save_config(&config, &cfg_path)?;

        if auto_commit {
            let attrs_blob = stage_attrs_blob(&repo)?;
            let vendors_blob = stage_gitvendors(&repo, config_str.as_bytes())?;
            let tree = final_tree(&repo, full_tree, attrs_blob, vendors_blob)?;
            commit_and_advance(&repo, &entry, &merge, tree, current_head, &msg)?;
            current_head = repo
                .head_commit()
                .map(|c| c.id().detach())
                .map_err(|e| format!("HEAD after commit: {e}"))?;
            eprintln!("Updated {n}.");
        } else {
            stage_gitvendors(&repo, config_str.as_bytes())?;
            repo.prepare_merge(&entry, &merge, &msg)?;
            eprintln!("Updated {n}. Run `git commit` to record the merge.");
        }
    }

    Ok(())
}

fn cmd_status(name: Option<String>, fetch: bool) -> Result<()> {
    let repo = discover()?;
    let cfg_path = config_path(&repo)?;
    let config = load_config(&cfg_path)?;

    let entries: Vec<VendorEntry> = match name {
        Some(ref n) => vec![require_entry(&config, n)?],
        None => config.entries()?,
    };

    if entries.is_empty() {
        eprintln!("No vendors configured.");
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
        println!("{}\t{}\t{label}", entry.name, entry.url);
    }

    Ok(())
}

fn cmd_remove(name: String, keep_files: bool) -> Result<()> {
    let repo = discover()?;
    let cfg_path = config_path(&repo)?;
    let mut config = load_config(&cfg_path)?;

    let entry = require_entry(&config, &name)?;

    if !keep_files {
        let head_oid = repo.head_commit().ok().map(|c| c.id().detach());

        if let Some(oid) = head_oid {
            use gix::bstr::ByteSlice as _;
            let workdir = repo.workdir().ok_or("not a working-copy repository")?;
            let paths = repo.vendor_paths(&entry, oid)?;
            for p in &paths {
                let abs = workdir.join(gix::path::from_bstr(p).as_ref());
                if abs.symlink_metadata().is_ok() {
                    std::fs::remove_file(&abs)?;
                }
            }
            let path_refs: Vec<&gix::bstr::BStr> = paths.iter().map(|b| b.as_ref()).collect();
            repo.untrack_vendor(&entry, &path_refs)?;

            // Remove the deleted vendor files from the index so `git commit`
            // records the deletions rather than leaving them tracked.
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
    save_config(&config, &cfg_path)?;
    eprintln!("Removed vendor {name}.");
    Ok(())
}

fn cmd_list() -> Result<()> {
    let repo = discover()?;
    let cfg_path = config_path(&repo)?;
    let config = load_config(&cfg_path)?;
    let entries = config.entries()?;

    if entries.is_empty() {
        eprintln!("No vendors configured.");
        return Ok(());
    }

    for entry in &entries {
        let ref_label = entry.ref_name.as_deref().unwrap_or("HEAD");
        let mode_label = entry.mode.as_str();
        println!("{}\t{}\t{ref_label}\t{mode_label}", entry.name, entry.url);
    }

    Ok(())
}
