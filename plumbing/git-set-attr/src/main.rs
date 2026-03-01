use clap::{CommandFactory, Parser};
use git_set_attr::cli::Cli;
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

    if let Err(e) = git_set_attr::exe::run(&cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
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

    let man_path = man1_dir.join("git-set-attr.1");
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
