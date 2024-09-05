use super::validate_dependency;
use clap::Parser;
use cliclack::log::{remark, warning};
use soldeer_core::{
    errors::PublishError,
    push::{filter_files_to_copy, prompt_user_for_confirmation, push_version, validate_name},
    utils::check_dotfiles,
    Result,
};
use std::{env, path::PathBuf};

/// Push a dependency to the repository
#[derive(Debug, Clone, Parser)]
#[clap(
    long_about = "Push a Dependency to the Repository

Examples:
- Current directory: soldeer push mypkg~0.1.0
- Custom directory: soldeer push mypkg~0.1.0 /path/to/dep
- Dry run: soldeer push mypkg~0.1.0 --dry-run

To ignore certain files, create a `.soldeerignore` file in the root of the project and add the files you want to ignore. The `.soldeerignore` uses the same syntax as `.gitignore`.",
    after_help = "For more information, read the README.md"
)]
pub struct Push {
    /// The dependency name and version, separated by a tilde.
    ///
    /// This should always be used when you want to push a dependency to the central repository: `<https://soldeer.xyz>`.
    #[arg(value_parser = validate_dependency, value_name = "DEPENDENCY>~<VERSION")]
    pub dependency: String,

    /// Use this if the package you want to push is not in the current directory.
    ///
    /// Example: `soldeer push mypkg~0.1.0 /path/to/dep`.
    pub path: Option<PathBuf>,

    /// If set, does not publish the package but generates a zip file that can be inspected.
    #[arg(short, long, default_value_t = false)]
    pub dry_run: bool,

    /// Use this if you want to skip the warnings that can be triggered when trying to push
    /// dotfiles like .env.
    #[arg(long, default_value_t = false)]
    pub skip_warnings: bool,
}

pub(crate) async fn push_command(cmd: Push) -> Result<()> {
    let path = cmd.path.unwrap_or(env::current_dir()?);

    let files_to_copy: Vec<PathBuf> = filter_files_to_copy(&path);

    // Check for sensitive files or directories
    if !cmd.dry_run &&
        !cmd.skip_warnings &&
        check_dotfiles(&files_to_copy) &&
        !prompt_user_for_confirmation()?
    {
        return Err(PublishError::UserAborted.into());
    }

    if cmd.dry_run {
        remark("Running in dry-run mode, a zip file will be created for inspection")?;
    }

    if cmd.skip_warnings {
        warning("Sensitive file warnings are being ignored as requested")?;
    }

    let (dependency_name, dependency_version) =
        cmd.dependency.split_once('~').expect("dependency string should have name and version");

    validate_name(dependency_name)?;

    push_version(dependency_name, dependency_version, path, &files_to_copy, cmd.dry_run).await?;
    Ok(())
}
