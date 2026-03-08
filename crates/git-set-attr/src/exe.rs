use crate::{SetAttr, cli::Cli};
use std::path::PathBuf;

/// Resolve the `.gitattributes` path to write to.
///
/// If the user supplied `--file`, that path is used as-is.  Otherwise we
/// default to `<cwd>/.gitattributes`, falling back to `<workdir>/.gitattributes`
/// if the current directory is outside the repository's working tree.
fn resolve_gitattributes_path(
    repo: &crate::Repository,
    explicit: Option<&std::path::Path>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    let workdir = repo
        .workdir()
        .ok_or("repository has no working directory")?;

    let cwd = std::env::current_dir()?;

    if let Ok(relative) = cwd.strip_prefix(workdir) {
        Ok(workdir.join(relative).join(".gitattributes"))
    } else {
        Ok(workdir.join(".gitattributes"))
    }
}

pub fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = crate::Repository::open(".")?;

    let gitattributes = resolve_gitattributes_path(&repo, cli.file.as_deref())?;
    let attributes: Vec<&str> = cli.attributes.iter().map(|s| s.as_str()).collect();

    repo.set_attr(&cli.pattern, &attributes, &gitattributes)?;

    Ok(())
}
