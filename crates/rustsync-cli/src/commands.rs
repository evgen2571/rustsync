use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
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
