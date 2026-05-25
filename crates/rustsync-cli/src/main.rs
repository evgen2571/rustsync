use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rustsync")]
struct CLI {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new project
    Init { name: String },
    /// Push changes/files to the server
    Push { filename: String },
    /// Pull changes/files from the server
    Pull { filename: String },
    /// Encrypt file
    Encrypt { filename: String },
    /// Decrypt file
    Decrypt { filename: String },
}

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
