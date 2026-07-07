use crate::cli::{Cli, Command, DeviceCommand};
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
        Command::Device { command } => match command {
            DeviceCommand::Request {
                path,
                workspace_id,
                device_name,
            } => commands::device::request(path, workspace_id, device_name).await?,
            DeviceCommand::ListRequests { path } => commands::device::list_requests(path).await?,
            DeviceCommand::Approve {
                path,
                join_request_id,
                role,
            } => commands::device::approve(path, join_request_id, role).await?,
            DeviceCommand::List { path } => commands::device::list(path).await?,
        },
    }

    Ok(())
}
