use crate::{SetAttr, cli::Cli};

pub fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = crate::Repository::open(".")?;

    let attributes: Vec<&str> = cli.attributes.iter().map(|s| s.as_str()).collect();

    repo.set_attr(&cli.pattern, &attributes, cli.file.as_deref())?;

    Ok(())
}
