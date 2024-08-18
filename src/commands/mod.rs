use crate::SoldeerError;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod init;
pub mod install;
pub mod update;

pub type Result<T> = std::result::Result<T, SoldeerError>;

/// A minimal solidity dependency manager.
#[derive(Parser, Debug)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Subcommands {
    Init(init::Init),
    Install(install::Install),
    Update(update::Update),
    Login(Login),
    Push(Push),
    Uninstall(Uninstall),
    Version(Version),
}

fn validate_dependency(dep: &str) -> std::result::Result<String, String> {
    if dep.split('~').count() != 2 {
        return Err("The dependency should be in the format <DEPENDENCY>~<VERSION>".to_string());
    }
    Ok(dep.to_string())
}

/// Display the version of Soldeer
#[derive(Debug, Clone, Parser)]
pub struct Version {}

/// Log into the central repository to push the dependencies
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Login {}

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

/// Uninstall a dependency
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Uninstall {
    /// The dependency name. Specifying a version is not necessary.
    pub dependency: String,
}
