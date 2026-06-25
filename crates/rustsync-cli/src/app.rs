use crate::cli::{Cli, Command};
use crate::commands;

use clap::Parser;
use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { path } => commands::init::run(path)?,
        Command::Status { path } => commands::status::run(path)?,
    }

    Ok(())
}
