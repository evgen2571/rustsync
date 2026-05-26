use crate::commands::Commands;
use clap::Parser;

#[derive(Parser)]
#[command(name = "rustsync")]
pub struct CLI {
    #[command(subcommand)]
    pub command: Commands,
}
