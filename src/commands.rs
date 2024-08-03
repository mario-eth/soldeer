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
    VersionDryRun(VersionDryRun),
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Initialize a new Soldeer project for use with Foundry.
Use --clean true if you want to delete .gitmodules and lib directory that were created in Foundry.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer init"
)]
pub struct Init {
    #[arg(long, value_parser = clap::value_parser!(bool))]
    pub clean: Option<bool>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Installing a Dependency

You can install a dependency from the Soldeer repository, a custom URL pointing to a zip file, or from Git using a Git link.

**Important:** The `~` symbol when specifying the dependency is crucial to differentiate between the name and the version that needs to be installed.

- **Example from Soldeer repository:** 
  soldeer install @openzeppelin-contracts~2.3.0

- **Example from a custom URL:** 
  soldeer install @openzeppelin-contracts~2.3.0 https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v5.0.2.zip

- **Example from Git:** 
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git

- **Example from Git with a specified commit:** 
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --rev 05f218fb6617932e56bf5388c3b389c3028a7b73

If you want to regenerate the remappings from scratch, use
  --reg-remappings true
",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer install <DEPENDENCY>~<VERSION> [URL]"
)]
pub struct Install {
    #[clap(required = false)]
    #[clap(
        help = "Use this as [NAME]~[VERSION]. This should be always used if you want to install a certain dependency from: remote/custom url/git"
    )]
    pub dependency: Option<String>,
    #[clap(required = false)]
    #[arg(long, value_parser = clap::value_parser!(String))]
    #[clap(
        help = "Use this if your dependency is stored at a specific link in a zip file, e.g., https://my-domain/dep.zip."
    )]
    pub remote_url: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(String))]
    #[clap(
        help = "Set this to true if you want to specify a certain commit when installing a dependency from a Git repository."
    )]
    pub rev: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    #[clap(
        help = "Use this option set to true if you want to regenerate the remappings from scratch. This will delete the old remappings."
    )]
    pub reg_remappings: Option<bool>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Update dependencies by reading the config file.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer update"
)]
pub struct Update {
    #[arg(long, value_parser = clap::value_parser!(bool))]
    /// This regenerates the remappings from scratch, will delete old remappings and regenerate
    /// them
    #[clap(
        help = "Use this option set to true if you want to regenerate the remappings from scratch. This will delete the old remappings."
    )]
    pub reg_remappings: Option<bool>,
}

#[derive(Debug, Clone, Parser)]
pub struct VersionDryRun {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Log in to the central repository to push the dependencies.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer login"
)]
pub struct Login {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Push a Dependency to the Repository
The `PATH_TO_DEPENDENCY` is optional. If not provided, the current directory will be used.

**Example:** 
- If the current directory is `/home/soldeer/my_project` and you do not specify the `PATH_TO_DEPENDENCY`, the files inside `/home/soldeer/my_project` will be pushed to the repository.
- If you specify the `PATH_TO_DEPENDENCY`, the files inside the specified directory will be pushed to the repository.

To ignore certain files, create a `.soldeerignore` file in the root of the project and add the files you want to ignore. The `.soldeerignore` works like a `.gitignore`.

For a dry run, use the `--dry-run` argument set to `true`: `soldeer push ... --dry-run true`. This will create a zip file that you can inspect to see what will be pushed to the central repository.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer push <DEPENDENCY>~<VERSION> [PATH_TO_DEPENDENCY]"
)]
pub struct Push {
    #[clap(required = true)]
    #[clap(
        help = "Use this format: `[NAME]~[VERSION]`. This should always be used when you want to push a dependency to the central repository: https://soldeer.xyz."
    )]
    pub dependency: String,

    #[clap(
        help = "Use this if the dependency you want to push is not in the current directory. For example: `soldeer push /path/to/dep`."
    )]
    pub path: Option<PathBuf>,
    #[clap(
        help = "Use this if you want to run a dry run. This will generate a zip file that you can inspect to see what will be pushed."
    )]
    #[arg(short, long)]
    pub dry_run: Option<bool>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    #[clap(
        help = "Use this if you want to skip the warnings that can be triggered when trying to push .dot files like .env."
    )]
    pub skip_warnings: Option<bool>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Uninstall a dependency. soldeer uninstall <DEPENDENCY>",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer uninstall <DEPENDENCY>"
)]
pub struct Uninstall {
    #[clap(required = true)]
    #[clap(
        help = "Use this command as `soldeer uninstall [NAME]`. Specifying a version is unnecessary because there can only be one dependency with a given name"
    )]
    pub dependency: String,
}
