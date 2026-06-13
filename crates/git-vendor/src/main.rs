#![allow(missing_docs)]

mod cli;
mod exe;

use clap::Parser as _;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() {
    if let Err(e) = run() {
        if e.downcast_ref::<exe::ConflictExit>().is_none() {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    exe::Executor::discover()?.run(cli::Cli::parse(), &mut exe::Io::stdio())
}
