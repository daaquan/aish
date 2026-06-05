// SPDX-License-Identifier: MIT
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
    /// Emit machine-readable JSON instead of human text (for CI/CD).
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
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
    /// Manage tool plugins (install / update / list / enable / disable / uninstall).
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Any other subcommand is dispatched to an installed plugin.
    #[command(external_subcommand)]
    External(Vec<String>),
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

#[derive(Subcommand)]
pub enum PluginAction {
    /// Build and install a plugin from the registry.
    Install {
        /// Plugin name (directory in the registry).
        name: String,
        /// Skip the trusted-code confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Rebuild and reinstall a plugin from the registry (or all if no name).
    Update {
        /// Plugin to update; omit to update every installed plugin.
        name: Option<String>,
    },
    /// List installed plugins.
    List,
    /// Enable an installed plugin.
    Enable { name: String },
    /// Disable an installed plugin (keeps it installed).
    Disable { name: String },
    /// Remove an installed plugin.
    Uninstall { name: String },
}
