use clap::{CommandFactory, Parser};
use git_vendor::Vendor;
use git_vendor::cli::{self, Cli, Command};
use git_vendor::exe;
use std::path::PathBuf;
use std::process;

fn main() {
    if let Some(dir) = parse_generate_man_flag() {
        if let Err(e) = generate_man_page(dir) {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Determine which vendors to merge based on `name`, `--all`, and how many
/// vendors are configured.  Returns the list of vendor names to operate on.
fn resolve_merge_targets(
    repo: &git2::Repository,
    name: &Option<String>,
    all: bool,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    match name {
        Some(n) => Ok(vec![n.clone()]),
        None => {
            let vendors = repo.list_vendors()?;
            if vendors.is_empty() {
                return Ok(vec![]);
            }
            if all || vendors.len() == 1 {
                Ok(vendors.into_iter().map(|v| v.name).collect())
            } else {
                Err(format!(
                    "multiple vendors configured; specify a name or pass --all\n\
                     \n  configured vendors: {}",
                    vendors
                        .iter()
                        .map(|v| v.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .into())
            }
        }
    }
}

/// Print merge outcomes to stderr.
fn print_merge_outcomes(outcomes: &[(String, exe::MergeOutcome)]) {
    for (vname, outcome) in outcomes {
        match outcome {
            exe::MergeOutcome::UpToDate { .. } => {
                eprintln!("'{}' is already up to date.", vname);
            }
            exe::MergeOutcome::Clean { vendor } => {
                eprintln!(
                    "'{}' merged cleanly (base {}).",
                    vname,
                    vendor.base.as_deref().unwrap_or("(none)"),
                );
            }
            exe::MergeOutcome::Conflict { .. } => {
                eprintln!(
                    "Conflicts detected for '{}'. Resolve them and commit.",
                    vname,
                );
            }
        }
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = exe::open_repo(cli.repo.as_deref())?;

    match &cli.command {
        Command::List => {
            let vendors = exe::list(&repo)?;
            if vendors.is_empty() {
                println!("No vendors configured.");
            } else {
                for v in &vendors {
                    let branch = v.branch.as_deref().unwrap_or("(default)");
                    let base = v.base.as_deref().unwrap_or("(none)");
                    let path = v.path.as_deref().unwrap_or("(root)");
                    println!(
                        "{}\n  url:    {}\n  branch: {}\n  base:   {}\n  path:   {}",
                        v.name, v.url, branch, base, path,
                    );
                }
            }
        }

        Command::Add {
            url,
            name,
            branch,
            pattern,
            path,
            strategy_option,
        } => {
            let file_favor = match strategy_option {
                cli::StrategyOption::Normal => None,
                other => Some(other.to_file_favor()),
            };
            let name = name.as_deref().unwrap_or_else(|| cli::name_from_url(url));
            let patterns: Vec<&str> = pattern.iter().map(String::as_str).collect();
            let outcome = exe::add(
                &repo,
                name,
                url,
                branch.as_deref(),
                &patterns,
                path.as_deref(),
                file_favor,
            )?;
            match outcome {
                exe::MergeOutcome::UpToDate { .. } => unreachable!("add never produces UpToDate"),
                exe::MergeOutcome::Clean { vendor } => {
                    eprintln!(
                        "Added vendor '{}' (base {}).",
                        vendor.name,
                        vendor.base.as_deref().unwrap_or("(none)"),
                    );
                }
                exe::MergeOutcome::Conflict { vendor, .. } => {
                    eprintln!(
                        "Added vendor '{}' (base {}) with conflicts",
                        vendor.name,
                        vendor.base.as_deref().unwrap_or("(none)"),
                    );
                }
            }
        }

        Command::Fetch { name } => match name {
            Some(n) => {
                if let Some(oid) = exe::fetch_one(&repo, n)? {
                    eprintln!("Fetched '{}' -> {}", n, oid);
                }
            }
            None => {
                if repo.list_vendors()?.is_empty() {
                    println!("No vendors configured.");
                } else {
                    for (vname, oid) in &exe::fetch_all(&repo)? {
                        eprintln!("Fetched '{}' -> {}", vname, oid);
                    }
                }
            }
        },

        Command::Track { name, pattern } => {
            let patterns: Vec<&str> = pattern.iter().map(String::as_str).collect();
            exe::track(&repo, name, &patterns)?;
            for p in &patterns {
                eprintln!("Tracking '{}' for vendor '{}'.", p, name);
            }
        }

        Command::Untrack { name, pattern } => {
            let patterns: Vec<&str> = pattern.iter().map(String::as_str).collect();
            exe::untrack(&repo, name, &patterns)?;
            for p in &patterns {
                eprintln!("Untracking '{}' for vendor '{}'.", p, name);
            }
        }

        Command::Rm { name } => {
            exe::rm(&repo, name)?;
            eprintln!("Removed vendor '{}'.", name);
            eprintln!("Vendored files are marked as conflicts. Resolve with:");
            eprintln!("  git rm <file>    # accept deletion");
            eprintln!("  git add <file>   # keep file");
        }

        Command::Status => {
            let statuses = exe::status(&repo)?;
            if statuses.is_empty() {
                println!("No vendors configured.");
            } else {
                let mut any_updates = false;
                for s in &statuses {
                    match s.upstream_oid {
                        Some(oid) => {
                            println!("{}: upstream updated ({})", s.name, oid);
                            any_updates = true;
                        }
                        None => println!("{}: up to date", s.name),
                    }
                }
                if !any_updates && statuses.len() > 1 {
                    println!("\nAll vendors are up to date.");
                }
            }
        }

        Command::Prune => {
            let pruned = exe::prune(&repo)?;
            if pruned.is_empty() {
                println!("No orphaned vendor refs found.");
            } else {
                for name in &pruned {
                    eprintln!("Pruned refs/vendor/{}/{{head,base}}", name);
                }
            }
        }

        Command::Merge {
            name,
            all,
            strategy_option,
        } => {
            let file_favor = match strategy_option {
                cli::StrategyOption::Normal => None,
                other => Some(other.to_file_favor()),
            };
            let targets = resolve_merge_targets(&repo, name, *all)?;
            if targets.is_empty() {
                println!("No vendors configured.");
                return Ok(());
            }

            let mut outcomes = Vec::with_capacity(targets.len());
            for t in &targets {
                let outcome = exe::merge_one(&repo, t, file_favor)?;
                outcomes.push((t.clone(), outcome));
            }

            print_merge_outcomes(&outcomes);
        }

        Command::Pull {
            name,
            all,
            strategy_option,
        } => {
            let file_favor = match strategy_option {
                cli::StrategyOption::Normal => None,
                other => Some(other.to_file_favor()),
            };

            // Resolve targets first (requires name, --all, or single vendor).
            let targets = resolve_merge_targets(&repo, name, *all)?;
            if targets.is_empty() {
                println!("No vendors configured.");
                return Ok(());
            }

            // Fetch only the resolved targets.
            for t in &targets {
                if let Some(oid) = exe::fetch_one(&repo, t)? {
                    eprintln!("Fetched '{}' -> {}", t, oid);
                }
            }

            let mut outcomes = Vec::with_capacity(targets.len());
            for t in &targets {
                let outcome = exe::merge_one(&repo, t, file_favor)?;
                outcomes.push((t.clone(), outcome));
            }

            print_merge_outcomes(&outcomes);
        }
    }

    Ok(())
}

/// Check for `--generate-man <DIR>` before clap parses, so it doesn't
/// conflict with the required positional arguments.
fn parse_generate_man_flag() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--generate-man")?;
    let dir = args
        .get(pos + 1)
        .map(PathBuf::from)
        .unwrap_or_else(default_man_dir);
    Some(dir)
}

fn default_man_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".local/share")
        })
        .join("man")
}

fn generate_man_page(output_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let man1_dir = output_dir.join("man1");
    std::fs::create_dir_all(&man1_dir)?;

    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;

    let man_path = man1_dir.join("git-vendor.1");
    std::fs::write(&man_path, buffer)?;

    let output_dir = output_dir.canonicalize()?;
    eprintln!("Wrote man page to {}", man_path.canonicalize()?.display());

    if !manpath_covers(&output_dir) {
        eprintln!();
        eprintln!("You may need to add this to your shell environment:");
        eprintln!();
        eprintln!("  export MANPATH=\"{}:$MANPATH\"", output_dir.display());
    }
    Ok(())
}

/// Returns `true` if `dir` is equal to, or a subdirectory of, any component
/// in the `MANPATH` environment variable.
fn manpath_covers(dir: &std::path::Path) -> bool {
    let Some(manpath) = std::env::var_os("MANPATH") else {
        return false;
    };
    for component in std::env::split_paths(&manpath) {
        let Ok(component) = component.canonicalize() else {
            continue;
        };
        if dir.starts_with(&component) {
            return true;
        }
    }
    false
}
