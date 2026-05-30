mod actions;
mod cli;
mod commands;

use actions::{decrypt::decrypt, encrypt::encrypt, init::init, pull::pull, push::push};
use clap::Parser;
use cli::Cli;
use commands::Commands;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name } => {
            init(name);
        }
        Commands::Push { filename } => {
            push(filename);
        }
        Commands::Pull { filename } => {
            pull(filename);
        }
        Commands::Encrypt { filename } => {
            encrypt(filename);
        }
        Commands::Decrypt { filename } => {
            decrypt(filename);
        }
    }
}
