// SPDX-License-Identifier: MIT
use aish::cli::Cli;
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    if let Err(e) = aish::commands::run(cli).await {
        if verbose {
            eprintln!("error: {e:?}");
        } else {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}
