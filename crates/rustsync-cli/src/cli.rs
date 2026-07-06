use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "rustsync")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Add {
        #[arg(short = 'A', long = "all")]
        all: bool,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Push {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Pull {
        #[arg(long)]
        force: bool,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}
