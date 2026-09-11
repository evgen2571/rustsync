use crate::cli::{Cli, Command, DeviceCommand};
use crate::commands;

use clap::{CommandFactory, Parser};
use std::error::Error;

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn Error>> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let server_url = &cli.server_url;

    match cli.command {
        Command::Init { path } => commands::init::run(path, server_url).await?,
        Command::Version => println!("rustsync {} (protocol 1)", env!("CARGO_PKG_VERSION")),
        Command::Completions { shell } => clap_complete::generate(
            shell,
            &mut Cli::command(),
            "rustsync",
            &mut std::io::stdout(),
        ),
        Command::Status { path, json } => commands::status::report(path, server_url, json).await?,

        Command::RemoteStatus { path } => commands::sync::remote_status(path, server_url).await?,
        Command::Sync {
            path,
            dry_run,
            discard_local,
            yes: _,
            json,
            quiet,
            no_progress,
        } => {
            commands::sync::sync(
                path,
                dry_run,
                discard_local,
                server_url,
                commands::sync::OutputOptions {
                    json,
                    quiet,
                    no_progress,
                },
            )
            .await?
        }
        Command::Conflicts { path } => commands::sync::conflicts(path)?,
        Command::Resolve {
            path,
            keep_local,
            keep_remote,
            workspace,
        } => commands::sync::resolve(workspace, path, keep_local, keep_remote)?,
        Command::Doctor { path, json } => commands::sync::doctor(path, server_url, json).await?,
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

/// Run the command and render any failure in the selected output format.
pub fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if !error.is::<ReportedError>() {
                if std::env::args_os().any(|arg| arg == "--json") {
                    println!(
                        "{}",
                        serde_json::json!({"ok": false, "error": error.to_string()})
                    );
                } else {
                    eprintln!("error: {error}");
                }
            }
            std::process::ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReportedError;
impl std::fmt::Display for ReportedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command failed; see report")
    }
}
impl Error for ReportedError {}
