use std::path::PathBuf;

use clap::Parser;

/// Merge strategy option for resolving conflicting regions during vendor
/// merges.  These mirror the `-X` / `--strategy-option` values accepted by
/// `git merge`.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum StrategyOption {
    /// Record conflicts in the index so that checkout produces conflict
    /// markers in the working directory (the default).
    #[default]
    Normal,
    /// Resolve conflicts by taking "ours" (the local side).
    Ours,
    /// Resolve conflicts by taking "theirs" (the upstream/vendor side).
    Theirs,
    /// Combine both sides, keeping each unique line (union merge).
    Union,
}

impl StrategyOption {
    /// Convert to the corresponding `git2::FileFavor`.
    pub fn to_file_favor(self) -> git2::FileFavor {
        match self {
            StrategyOption::Normal => git2::FileFavor::Normal,
            StrategyOption::Ours => git2::FileFavor::Ours,
            StrategyOption::Theirs => git2::FileFavor::Theirs,
            StrategyOption::Union => git2::FileFavor::Union,
        }
    }
}

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

        /// Glob pattern(s) selecting which upstream files to vendor.
        #[arg(short, long, default_value = "**")]
        pattern: Vec<String>,

        /// Local directory where vendored files are placed (defaults to current directory).
        #[arg(long)]
        path: Option<PathBuf>,

        /// Strategy option for resolving conflicting regions during the merge.
        #[arg(short = 'X', long = "strategy-option", value_enum, default_value_t)]
        strategy_option: StrategyOption,
    },

    /// Fetch the latest upstream commits for one or all vendors.
    Fetch {
        /// Vendor name. If omitted, fetches all vendors.
        name: Option<String>,
    },

    /// Remove a vendor source and its associated refs and attributes.
    Rm {
        /// Vendor name to remove.
        name: String,
    },

    /// Track files or patterns for a vendor.
    ///
    /// Without `--pattern`: writes `vendor=<name>` entries into `.gitattributes`
    /// for the given file paths.
    ///
    /// With `--pattern`: adds glob pattern(s) to the vendor's `patterns` list
    /// in `.gitvendors`.
    Track {
        /// Vendor name.
        name: String,

        /// File path(s) to attribute (used without --pattern).
        #[arg(conflicts_with = "pattern")]
        paths: Vec<String>,

        /// Glob pattern(s) to add to the vendor's pattern list in `.gitvendors`.
        #[arg(short, long)]
        pattern: Vec<String>,
    },

    /// Untrack files or patterns for a vendor.
    ///
    /// Without `--pattern`: removes `vendor=<name>` entries from `.gitattributes`
    /// for the given file paths.
    ///
    /// With `--pattern`: removes glob pattern(s) from the vendor's `patterns` list
    /// in `.gitvendors`.
    Untrack {
        /// Vendor name.
        name: String,

        /// File path(s) to unattribute (used without --pattern).
        #[arg(conflicts_with = "pattern")]
        paths: Vec<String>,

        /// Glob pattern(s) to remove from the vendor's pattern list in `.gitvendors`.
        #[arg(short, long)]
        pattern: Vec<String>,
    },

    /// Show which vendors have unmerged upstream changes.
    Status,

    /// Clean up refs/vendor/*/head (and /base) refs that have no corresponding entry in .gitvendors.
    Prune,

    /// Merge upstream changes for a vendor.
    Merge {
        /// Vendor name. Required unless `--all` is given or only one vendor
        /// is configured.
        name: Option<String>,

        /// Merge all configured vendors.
        #[arg(short, long)]
        all: bool,

        /// Strategy option for resolving conflicting regions during the merge.
        #[arg(short = 'X', long = "strategy-option", value_enum, default_value_t)]
        strategy_option: StrategyOption,

        /// Stage the merge result without creating a commit. Incompatible with
        /// the `replay` commit mode.
        #[arg(long)]
        no_commit: bool,
    },

    /// Fetch and merge upstream changes for a vendor.
    Pull {
        /// Vendor name. Required unless `--all` is given or only one vendor
        /// is configured.
        name: Option<String>,

        /// Pull all configured vendors.
        #[arg(short, long)]
        all: bool,

        /// Strategy option for resolving conflicting regions during the merge.
        #[arg(short = 'X', long = "strategy-option", value_enum, default_value_t)]
        strategy_option: StrategyOption,

        /// Stage the merge result without creating a commit. Incompatible with
        /// the `replay` commit mode.
        #[arg(long)]
        no_commit: bool,
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
