use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "git vendor", bin_name = "git vendor")]
#[command(
    author,
    version,
    about = "An in-source vendoring alternative to Git submodules and subtrees.",
    long_about = None
)]
pub struct Cli {
    /// Path to the git repository. Defaults to the current directory.
    #[arg(short = 'C', long, global = true)]
    pub repo: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// List all configured vendor sources.
    List,

    /// Add a new vendor source.
    Add {
        /// A unique name for this vendor (used in config keys and ref names).
        name: String,

        /// The remote URL to vendor from.
        url: String,

        /// The upstream branch to track (defaults to HEAD).
        #[arg(short, long)]
        branch: Option<String>,

        /// Glob pattern selecting which upstream files to vendor.
        #[arg(short, long, default_value = "**")]
        pattern: String,

        /// Local directory where vendored files are placed.
        #[arg(short, long)]
        local_root: Option<PathBuf>,
    },

    /// Fetch the latest upstream commits for one or all vendors.
    Fetch {
        /// Vendor name. If omitted, fetches all vendors.
        name: Option<String>,
    },

    /// Check which vendors have unmerged upstream changes.
    Check,

    /// Merge upstream changes for one or all vendors.
    Merge {
        /// Vendor name. If omitted, merges all vendors.
        name: Option<String>,
    },
}
