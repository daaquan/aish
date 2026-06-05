// SPDX-License-Identifier: AGPL-3.0-only
use clap::{Parser, Subcommand};

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
    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Generate a commit message from staged changes and optionally commit.
    Commit {
        /// Commit immediately without confirmation.
        #[arg(long)]
        apply: bool,
        /// Override the model alias from config.
        #[arg(long)]
        model: Option<String>,
        /// Override commit style (e.g. conventional).
        #[arg(long)]
        style: Option<String>,
        /// Override output language.
        #[arg(long)]
        lang: Option<String>,
        /// Add a DCO Signed-off-by trailer to the commit.
        #[arg(long)]
        signoff: bool,
        /// Bypass the response cache (force a fresh model request).
        #[arg(long)]
        no_cache: bool,
    },
    /// Write a commented config template to ~/.aish/config.yaml.
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
}

#[derive(Subcommand)]
pub enum ConfigAction {
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Validate the config and report problems without making any requests.
    Check,
}

#[derive(Subcommand)]
pub enum ProvidersAction {
    List,
}

#[derive(Subcommand)]
pub enum ModelsAction {
    List,
}
