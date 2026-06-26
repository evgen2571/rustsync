use rustsync_core::workspace::LocalWorkspaceEngine;
use std::{error::Error, path::PathBuf};

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let engine = LocalWorkspaceEngine::open(&path)?;
    let report = engine.stage_all()?;

    println!(
        "staged {} added, {} modified, {} deleted path(s) for push",
        report.summary.added, report.summary.modified, report.summary.deleted
    );

    Ok(())
}
