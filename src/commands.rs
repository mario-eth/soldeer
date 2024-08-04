use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A minimal solidity dependency manager.
#[derive(Parser, Debug)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Subcommands {
    Init(Init),
    Install(Install),
    Update(Update),
    Login(Login),
    Push(Push),
    Uninstall(Uninstall),
    Version(Version),
}

/// Initialize a new Soldeer project for use with Foundry
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Init {
    /// Clean the Foundry project by removing .gitmodules and the lib directory
    #[arg(long, default_value_t = false)]
    pub clean: bool,
}

fn validate_dependency(dep: &str) -> Result<String, String> {
    if dep.split('~').count() != 2 {
        return Err("The dependency should be in the format <DEPENDENCY>~<VERSION>".to_string());
    }
    Ok(dep.to_string())
}

/// Install a dependency
#[derive(Debug, Clone, Parser)]
#[clap(
    long_about = "Install a dependency

You can install a dependency from the Soldeer repository, a custom URL pointing to a zip file, or from Git using a Git link.
**Important:** The `~` symbol when specifying the dependency is crucial to differentiate between the name and the version that needs to be installed.
- **Example from Soldeer repository:**
  soldeer install @openzeppelin-contracts~2.3.0
- **Example from a custom URL:**
  soldeer install @openzeppelin-contracts~2.3.0 https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v5.0.2.zip
- **Example from Git:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git
- **Example from Git with a specified commit:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --rev 05f218fb6617932e56bf5388c3b389c3028a7b73",
    after_help = "For more information, read the README.md"
)]
pub struct Install {
    /// The dependency name and version, separated by a tilde.
    ///
    /// If not present, this command will perform `soldeer update`
    #[arg(value_parser = validate_dependency, value_name = "DEPENDENCY>~<VERSION")]
    pub dependency: Option<String>,

    /// The URL to the dependency zip file, if not from the Soldeer repository
    ///
    /// Example: https://my-domain/dep.zip
    #[arg(value_name = "URL")]
    pub remote_url: Option<String>,

    /// The revision of the dependency, if from Git
    #[arg(long)]
    pub rev: Option<String>,

    /// If set, this command will delete the existing remappings and re-create them
    #[arg(long, default_value_t = false)]
    pub regenerate_remappings: bool,
}

/// Update dependencies by reading the config file
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Update {
    /// If set, this command will delete the existing remappings and re-create them
    #[arg(long, default_value_t = false)]
    pub regenerate_remappings: bool,
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
    /// Example: `soldeer push /path/to/dep`.
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
