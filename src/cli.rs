// SPDX-License-Identifier: MIT
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "aish",
    version,
    about = "AI-powered extensible shell for developers"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Print detailed error context.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
    /// Emit machine-readable JSON instead of human text (for CI/CD).
    #[arg(long, global = true)]
    pub json: bool,
}

/// Options shared by every model-backed command (commit, pr, review, …).
#[derive(Args)]
pub struct ModelOpts {
    /// Override the model alias from config.
    #[arg(long, short = 'm')]
    pub model: Option<String>,
    /// Override output language.
    #[arg(long, short = 'l')]
    pub lang: Option<String>,
    /// Bypass the response cache (force a fresh model request).
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Generate a commit message from staged changes and optionally commit.
    Commit {
        /// Commit immediately without confirmation.
        #[arg(long, short = 'y')]
        apply: bool,
        /// Stage all tracked changes first (git commit -a), like `git add -u`.
        #[arg(long, short = 'a')]
        all: bool,
        /// Open the editor pre-filled with the message (git commit -e); save to
        /// commit, leave empty to abort. Skips the interactive prompt.
        #[arg(long)]
        edit: bool,
        /// Override commit style (e.g. conventional).
        #[arg(long)]
        style: Option<String>,
        /// Add a DCO Signed-off-by trailer to the commit.
        #[arg(long)]
        signoff: bool,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Generate a PR title/body from the branch diff and optionally create the PR.
    Pr {
        /// Create the PR immediately via `gh pr create` without confirmation.
        #[arg(long, short = 'y')]
        apply: bool,
        /// Base branch to diff against (default: auto-detect).
        #[arg(long)]
        base: Option<String>,
        /// Create the PR as a draft (passthrough to `gh pr create --draft`).
        #[arg(long)]
        draft: bool,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Model-review the staged diff (or the branch diff with --branch).
    Review {
        /// Review the diff against the default branch instead of the staged diff.
        #[arg(long)]
        branch: bool,
        /// Base branch to diff against (implies --branch).
        #[arg(long)]
        base: Option<String>,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Generate CHANGELOG entries from commits between two refs.
    Changelog {
        /// Start of the commit range (default: the latest tag).
        #[arg(long)]
        from: Option<String>,
        /// End of the commit range (default: HEAD).
        #[arg(long)]
        to: Option<String>,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Ask a one-shot question; piped stdin is included as context.
    Ask {
        /// The question to ask.
        question: String,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Run a command and, if it fails, diagnose the failure with the model.
    ///
    /// Diagnoses and suggests a fix; it does not edit files or re-run anything.
    /// The wrapped command's exit code is always propagated.
    Fix {
        /// The command to run, e.g. `aish fix cargo test`. Flags after the
        /// command belong to the command, not to aish.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        cmd: Vec<String>,
        /// Run the command via `sh -c` so pipes, redirects, and globs work.
        #[arg(long)]
        shell: bool,
        /// Diagnose even when the command succeeds (exit 0).
        #[arg(long)]
        always: bool,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Turn a natural-language description into a shell command and run it.
    ///
    /// The generated command is shown behind a confirm/edit gate before it
    /// runs; `--yes` is the only path to no-prompt execution, and `--print`
    /// emits the command without running it.
    Run {
        /// Natural-language description of the desired command, e.g.
        /// `aish run delete all merged git branches`.
        #[arg(trailing_var_arg = true, required = true)]
        prompt: Vec<String>,
        /// Skip the confirm prompt and run the command immediately.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Print the generated command and exit without running it.
        #[arg(long)]
        print: bool,
        #[command(flatten)]
        common: ModelOpts,
    },
    /// Interactively configure providers and a default model.
    Setup {
        /// Restore the initial template config (backs up any existing file).
        #[arg(long)]
        repair: bool,
    },
    /// Validate the aish configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// List configured providers.
    Providers {
        #[command(subcommand)]
        action: ProvidersAction,
    },
    /// List model aliases.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Report token usage and estimated cost from the audit log.
    Usage,
    /// Print a shell completion script (bash, zsh, fish, elvish, powershell).
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
    /// Inspect or empty the response cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Update aish to the latest GitHub release (or a pinned tag).
    Update {
        /// Only report whether a newer release exists; nonzero exit if outdated.
        #[arg(long)]
        check: bool,
        /// Install a specific release tag instead of the latest (e.g. 0.5.0).
        #[arg(long)]
        version: Option<String>,
    },
    /// Remove the aish binary (and optionally the ~/.aish data dir).
    Uninstall {
        /// Also delete the data dir ($AISH_HOME, default ~/.aish): config, cache, audit log.
        #[arg(long)]
        purge: bool,
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Validate the config and report problems without making any requests.
    Check {
        /// After static validation, send one minimal request per configured
        /// provider to verify reachability and credentials.
        #[arg(long)]
        ping: bool,
    },
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Report entry count and total size of the cache.
    Stats,
    /// Delete all cached responses.
    Clear {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum ProvidersAction {
    List,
}

#[derive(Subcommand)]
pub enum ModelsAction {
    List,
}
