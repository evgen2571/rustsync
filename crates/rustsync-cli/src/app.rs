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

        Command::RemoteStatus { path } => commands::sync::remote_status(path, server_url).await?,
        Command::Sync {
            path,
            dry_run,
            discard_local,
            yes: _,
        } => commands::sync::sync(path, dry_run, discard_local, server_url).await?,
        Command::Conflicts { path } => commands::sync::conflicts(path)?,
        Command::Resolve {
            path,
            keep_local,
            keep_remote,
            workspace,
        } => commands::sync::resolve(workspace, path, keep_local, keep_remote)?,
        Command::Doctor { path } => commands::sync::doctor(path, server_url).await?,
        Command::Device { command } => match command {
            DeviceCommand::Remove { path, device_id } => {
                commands::device::remove(path, device_id, server_url).await?
            }
            DeviceCommand::SetRole {
                path,
                device_id,
                role,
            } => commands::device::set_role(path, device_id, role, server_url).await?,
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
