use crate::cli::{Cli, Command, DeviceCommand};
use crate::commands;

use clap::Parser;
use std::error::Error;

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let server_url = &cli.server_url;

    match cli.command {
        Command::Init { path } => commands::init::run(path, server_url).await?,
        Command::Status { path } => commands::status::run(path)?,
        Command::Add { path, all: _ } => commands::add::run(path)?,
        Command::Push { path } => commands::sync::push(path, server_url).await?,
        Command::Pull { path, force } => commands::sync::pull(path, force, server_url).await?,
        Command::Device { command } => match command {
            DeviceCommand::Request {
                path,
                workspace_id,
                device_name,
            } => commands::device::request(path, workspace_id, device_name, server_url).await?,
            DeviceCommand::ListRequests { path } => {
                commands::device::list_requests(path, server_url).await?
            }
            DeviceCommand::Approve {
                path,
                join_request_id,
                role,
            } => commands::device::approve(path, join_request_id, role, server_url).await?,
            DeviceCommand::Bootstrap { path, workspace_id } => {
                commands::device::bootstrap(path, workspace_id, server_url).await?
            }
            DeviceCommand::List { path } => commands::device::list(path, server_url).await?,
        },
    }

    Ok(())
}
