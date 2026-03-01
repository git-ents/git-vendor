use clap::Parser;

#[derive(Parser)]
#[command(name = "git vendor", bin_name = "git vendor")]
#[command(author, version, about = "An in-source vendoring alternative to Git submodules and subtrees", long_about = None)]
pub struct Cli {}
