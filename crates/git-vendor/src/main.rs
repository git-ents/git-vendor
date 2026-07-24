#![allow(missing_docs)]

mod cli;
mod exe;

use anyhow::Result;
use clap::Parser as _;

fn main() {
    if let Err(e) = run() {
        // A conflict has already staged the working tree and printed actionable
        // guidance to stderr, mirroring `git merge`: exit non-zero without the
        // `error:` prefix. Anything else is a genuine failure worth rendering.
        if !matches!(e.downcast_ref::<exe::Error>(), Some(exe::Error::Conflict)) {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    exe::Executor::discover()?.run(cli::Cli::parse(), &mut exe::Io::stdio())?;
    Ok(())
}
