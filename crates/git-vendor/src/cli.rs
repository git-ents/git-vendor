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
        /// The remote URL to vendor from.
        url: String,

        /// A unique name for this vendor (used in config keys and ref names).
        /// Defaults to the basename of the URL, minus any `.git` suffix.
        #[arg(short, long)]
        name: Option<String>,

        /// The upstream branch to track (defaults to HEAD).
        #[arg(short, long)]
        branch: Option<String>,

        /// Glob pattern selecting which upstream files to vendor.
        #[arg(short, long, default_value = "**")]
        pattern: String,

        /// Local directory where vendored files are placed (defaults to current directory).
        #[arg(long)]
        path: Option<PathBuf>,
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

/// Derive a vendor name from a URL by taking the last path component and
/// stripping a trailing `.git` suffix, if present.
///
/// ```
/// # use git_vendor::cli::name_from_url;
/// assert_eq!(name_from_url("https://github.com/org/repo.git"), "repo");
/// assert_eq!(name_from_url("https://github.com/org/repo"), "repo");
/// assert_eq!(name_from_url("git@github.com:org/my-lib.git"), "my-lib");
/// assert_eq!(name_from_url("/local/path/to/repo.git"), "repo");
/// ```
pub fn name_from_url(url: &str) -> &str {
    let url = url.trim_end_matches('/');
    let basename = url.rsplit_once('/').map_or(url, |(_, b)| b);
    // Also handle SCP-style URLs like `git@host:path/repo.git`
    let basename = basename.rsplit_once(':').map_or(basename, |(_, b)| b);
    basename.strip_suffix(".git").unwrap_or(basename)
}
