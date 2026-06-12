//! Command-line interface shape for `git-vendor`.

use clap::{Parser, Subcommand};

/// Manage vendored external repository content.
#[derive(Parser)]
#[command(name = "git-vendor", bin_name = "git vendor")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Add a new vendor dependency and integrate it into the current branch.
    ///
    /// Fetches the upstream ref, three-way merges it into the working tree, and
    /// mints a merge commit. Equivalent to `git subtree add` or a tracked
    /// `git submodule add` that copies files instead of linking a repo.
    Add {
        /// Remote URL of the upstream repository.
        url: String,

        /// Unique name for this vendor (used in `.gitvendors` and `.gitattributes`).
        /// Defaults to the last path component of the URL, stripped of `.git`.
        name: Option<String>,

        /// Branch, tag, or SHA to track on the upstream (defaults to `HEAD`).
        #[arg(long, value_name = "REF")]
        ref_name: Option<String>,

        /// File pattern used to filter the upstream vendor content with optional
        /// remapping into the working copy, e.g. `src/**:vendor/lib/`. May be
        /// repeated to select multiple paths.
        #[arg(long = "pattern", short = 'p', value_name = "GLOB[:DEST]")]
        patterns: Vec<String>,

        /// Record upstream history as a parentless squash commit instead of a
        /// full merge.
        #[arg(long)]
        squash: bool,

        /// Commit message (defaults to `vendor: add <name>`).
        #[arg(long, short = 'm', value_name = "MSG")]
        message: Option<String>,
    },

    /// Fetch and integrate upstream updates for one or all vendors.
    ///
    /// Equivalent to `git subtree pull` or `git submodule update --remote`.
    Update {
        /// Vendor name to update; updates all configured vendors if omitted.
        name: Option<String>,

        /// Commit message (defaults to `vendor: update <name>`).
        #[arg(long, short = 'm', value_name = "MSG")]
        message: Option<String>,

        /// Allow integrating an upstream that was force-pushed (rewound
        /// history). Without this flag, a force-push is reported as an error.
        #[arg(long)]
        force: bool,
    },

    /// Show synchronization status for one or all vendors.
    Status {
        /// Vendor name to check; checks all configured vendors if omitted.
        name: Option<String>,

        /// Fetch from upstream before reporting status.
        #[arg(long, short = 'f')]
        fetch: bool,
    },

    /// Remove a vendor dependency and its files from the working tree.
    Remove {
        /// Vendor name to remove.
        name: String,

        /// Remove the config entry and `.gitattributes` tracking but leave the
        /// vendored files in the working tree.
        #[arg(long)]
        keep_files: bool,
    },

    /// List all configured vendor dependencies.
    List,
}
