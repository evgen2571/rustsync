mod cli;
mod commands;

use clap::Parser;
use cli::CLI;
use commands::Commands;

fn main() {
    let cli = CLI::parse();

    match cli.command {
        Commands::Init { name } => {
            println!("Initializing project: {}", name);
            // ...
            println!("Project successfully initialized!");
        }
        Commands::Push { filename } => {
            println!("Pushing {} to the server", filename);
            // ...
            println!("Files successfully pushed!");
        }
        Commands::Pull { filename } => {
            println!("Pulling {} from the server", filename);
            // ...
            println!("Files successfully pulled!");
        }
        Commands::Encrypt { filename } => {
            println!("Encrypting file: {}", filename);
            // ...
            println!("File successfully encrypted!");
        }
        Commands::Decrypt { filename } => {
            println!("Decrypting file: {}", filename);
            // ...
            println!("File successfully decrypted!");
        }
    }
}
