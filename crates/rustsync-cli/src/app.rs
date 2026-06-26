use crate::cli::{Cli, Command};
use crate::commands;

use clap::Parser;
use std::error::Error;

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { path } => commands::init::run(path).await?,
        Command::Status { path } => commands::status::run(path)?,
        Command::Add { path, all: _ } => commands::add::run(path)?,
        Command::Push { path } => commands::sync::push(path).await?,
        Command::Pull { path } => commands::sync::pull(path).await?,
    }

    Ok(())
}
