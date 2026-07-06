use std::process::ExitCode;

fn main() -> ExitCode {
    match rustsync_cli::app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
