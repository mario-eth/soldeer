use crate::utils::success;
use clap::Parser;
use soldeer_core::{Result, config::Paths};
use std::fs;

/// Clean downloaded dependencies and generated artifacts
#[derive(Debug, Clone, Default, Parser, bon::Builder)]
#[builder(on(String, into))]
#[clap(after_help = "For more information, read the README.md")]
#[non_exhaustive]
pub struct Clean {
    // No options for basic implementation
}

pub(crate) fn clean_command(paths: &Paths, _cmd: &Clean) -> Result<()> {
    // Remove dependencies folder if it exists
    if paths.dependencies.exists() {
        fs::remove_dir_all(&paths.dependencies)?;
        success!("Dependencies folder removed");
    }

    // Remove lock file if it exists
    if paths.lock.exists() {
        fs::remove_file(&paths.lock)?;
        success!("Lock file removed");
    }

    Ok(())
}
