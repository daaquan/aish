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
}

#[derive(Subcommand)]
pub enum ConfigAction {
    Init {
        #[arg(long)]
        force: bool,
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
