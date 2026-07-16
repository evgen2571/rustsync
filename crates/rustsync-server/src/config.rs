use std::path::PathBuf;

use clap::Parser;

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 3000;
pub const DEFAULT_STORAGE_DIR: &str = "./server-storage";

#[derive(Debug, Clone, Parser)]
#[command(name = "rustsync-server", about = "Run a RustSync server")]
pub struct ServerArgs {
    /// Address to bind to.
    #[arg(long, env = "RUSTSYNC_HOST", default_value = DEFAULT_HOST)]
    pub host: String,

    /// TCP port to listen on.
    #[arg(long, env = "RUSTSYNC_PORT", default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Directory used for server state and encrypted objects.
    #[arg(long, env = "RUSTSYNC_STORAGE_DIR", default_value = DEFAULT_STORAGE_DIR)]
    pub storage_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub storage_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            storage_dir: PathBuf::from(DEFAULT_STORAGE_DIR),
        }
    }
}

impl From<ServerArgs> for ServerConfig {
    fn from(args: ServerArgs) -> Self {
        Self {
            host: args.host,
            port: args.port,
            storage_dir: args.storage_dir,
        }
    }
}

impl ServerConfig {
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
