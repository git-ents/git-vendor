use clap::Parser;

#[derive(Parser)]
#[command(name = "git set-attr", bin_name = "git set-attr")]
#[command(author, version, about = "Set Git attributes via code, or from the command-line.", long_about = None)]
pub struct Cli {}
