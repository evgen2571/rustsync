use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use url::Url;

use crate::commands::sync::SERVER_BASE_URL;

#[derive(Debug, Parser)]
#[command(name = "rustsync")]
pub struct Cli {
    #[arg(long, global = true, default_value = SERVER_BASE_URL)]
    pub server_url: Url,

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
    RemoteStatus {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Sync {
        #[arg(long)]
        dry_run: bool,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Conflicts {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Resolve {
        path: String,
        #[arg(long, conflicts_with = "keep_remote")]
        keep_local: bool,
        #[arg(long, conflicts_with = "keep_local")]
        keep_remote: bool,
        #[arg(default_value = ".")]
        workspace: PathBuf,
    },
    Doctor {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    Request {
        workspace_id: rustsync_protocol::WorkspaceId,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        device_name: Option<String>,
    },
    ListRequests {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Approve {
        join_request_id: rustsync_protocol::JoinRequestId,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = DeviceRoleArg::Member)]
        role: DeviceRoleArg,
    },
    Bootstrap {
        workspace_id: rustsync_protocol::WorkspaceId,
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    List {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeviceRoleArg {
    Owner,
    Member,
}

impl From<DeviceRoleArg> for rustsync_protocol::WorkspaceRole {
    fn from(value: DeviceRoleArg) -> Self {
        match value {
            DeviceRoleArg::Owner => Self::Owner,
            DeviceRoleArg::Member => Self::Member,
        }
    }
}

impl std::fmt::Display for DeviceRoleArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owner => formatter.write_str("owner"),
            Self::Member => formatter.write_str("member"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_request_command() {
        let cli = Cli::parse_from([
            "rustsync",
            "device",
            "request",
            "workspace_test",
            "/tmp/pending",
            "--device-name",
            "laptop",
        ]);

        match cli.command {
            Command::Device {
                command:
                    DeviceCommand::Request {
                        workspace_id,
                        path,
                        device_name,
                    },
            } => {
                assert_eq!(workspace_id.as_str(), "workspace_test");
                assert_eq!(path, PathBuf::from("/tmp/pending"));
                assert_eq!(device_name.as_deref(), Some("laptop"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_device_approval_with_owner_role() {
        let cli = Cli::parse_from([
            "rustsync",
            "device",
            "approve",
            "join_test",
            "/tmp/workspace",
            "--role",
            "owner",
        ]);

        match cli.command {
            Command::Device {
                command:
                    DeviceCommand::Approve {
                        join_request_id,
                        path,
                        role,
                    },
            } => {
                assert_eq!(join_request_id.as_str(), "join_test");
                assert_eq!(path, PathBuf::from("/tmp/workspace"));
                assert!(matches!(role, DeviceRoleArg::Owner));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_device_bootstrap_command() {
        let cli = Cli::parse_from([
            "rustsync",
            "device",
            "bootstrap",
            "workspace_test",
            "/tmp/pending",
        ]);

        match cli.command {
            Command::Device {
                command: DeviceCommand::Bootstrap { workspace_id, path },
            } => {
                assert_eq!(workspace_id.as_str(), "workspace_test");
                assert_eq!(path, PathBuf::from("/tmp/pending"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_global_server_url_for_device_commands() {
        let cli = Cli::parse_from([
            "rustsync",
            "--server-url",
            "http://127.0.0.1:49152",
            "device",
            "list",
        ]);

        assert_eq!(cli.server_url.as_str(), "http://127.0.0.1:49152/");
        assert!(matches!(
            cli.command,
            Command::Device {
                command: DeviceCommand::List { .. }
            }
        ));
    }

    #[test]
    fn parses_sync_reconciliation_commands() {
        let sync = Cli::parse_from(["rustsync", "sync", "--dry-run", "/tmp/workspace"]);
        assert!(matches!(
            sync.command,
            Command::Sync { dry_run: true, path } if path.as_path() == std::path::Path::new("/tmp/workspace")
        ));

        let resolve = Cli::parse_from([
            "rustsync",
            "resolve",
            "notes.txt",
            "--keep-remote",
            "/tmp/workspace",
        ]);
        assert!(matches!(
            resolve.command,
            Command::Resolve { path, keep_local: false, keep_remote: true, workspace }
                if path == "notes.txt" && workspace.as_path() == std::path::Path::new("/tmp/workspace")
        ));
    }
}
