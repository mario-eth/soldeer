use crate::{
    errors::PublishError,
    push::{prompt_user_for_confirmation, push_version, validate_name},
    utils::{check_dotfiles_recursive, get_current_working_dir},
};

use super::{validate_dependency, Result};
use clap::Parser;
use cliclack::log::{remark, warning};
use std::path::PathBuf;

/// Push a dependency to the repository
#[derive(Debug, Clone, Parser)]
#[clap(
    long_about = "Push a Dependency to the Repository
The `PATH_TO_DEPENDENCY` is optional. If not provided, the current directory will be used.
**Example:**
- If the current directory is `/home/soldeer/my_project` and you do not specify the `PATH_TO_DEPENDENCY`, the files inside `/home/soldeer/my_project` will be pushed to the repository.
- If you specify the `PATH_TO_DEPENDENCY`, the files inside the specified directory will be pushed to the repository.
To ignore certain files, create a `.soldeerignore` file in the root of the project and add the files you want to ignore. The `.soldeerignore` works like a `.gitignore`.
For a dry run, use the `--dry-run` argument set to `true`: `soldeer push ... --dry-run true`. This will create a zip file that you can inspect to see what will be pushed to the central repository.",
    after_help = "For more information, read the README.md"
)]
pub struct Push {
    /// The dependency name and version, separated by a tilde.
    ///
    /// This should always be used when you want to push a dependency to the central repository: `<https://soldeer.xyz>`.
    #[arg(value_parser = validate_dependency, value_name = "DEPENDENCY>~<VERSION")]
    pub dependency: String,

    /// Use this if the dependency you want to push is not in the current directory.
    ///
    /// Example: `soldeer push mypkg~0.1.0 /path/to/dep`.
    pub path: Option<PathBuf>,

    /// Use this if you want to run a dry run. If set, this will generate a zip file that you can
    /// inspect to see what will be pushed.
    #[arg(short, long, default_value_t = false)]
    pub dry_run: bool,

    /// Use this if you want to skip the warnings that can be triggered when trying to push
    /// dotfiles like .env.
    #[arg(long, default_value_t = false)]
    pub skip_warnings: bool,
}

pub(crate) async fn push_command(cmd: Push) -> Result<()> {
    let path = cmd.path.unwrap_or(get_current_working_dir());

    // Check for sensitive files or directories
    if !cmd.dry_run &&
        !cmd.skip_warnings &&
        check_dotfiles_recursive(&path) &&
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

    push_version(dependency_name, dependency_version, path, cmd.dry_run).await?;
    Ok(())
}
