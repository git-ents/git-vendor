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
                    println!(
                        "{}\n  url:    {}\n  branch: {}\n  base:   {}",
                        v.name, v.url, branch, base,
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
            glob,
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
                *glob,
                file_favor,
            )?;
            match outcome {
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
                if !any_updates {
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
                    eprintln!("Pruned refs/vendor/{}", name);
                }
            }
        }

        Command::Merge {
            name,
            strategy_option,
        } => {
            let file_favor = match strategy_option {
                cli::StrategyOption::Normal => None,
                other => Some(other.to_file_favor()),
            };
            let outcomes = match name {
                Some(n) => {
                    let outcome = exe::merge_one(&repo, n, file_favor)?;
                    vec![(n.clone(), outcome)]
                }
                None => exe::merge_all(&repo, file_favor)?,
            };

            if outcomes.is_empty() {
                println!("No vendors configured.");
            } else {
                for (vname, outcome) in &outcomes {
                    match outcome {
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
                                vname
                            );
                        }
                    }
                }
            }
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
