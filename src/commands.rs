use clap::{
    Parser,
    Subcommand,
};

/// A minimal solidity dependency manager.
#[derive(Parser, Debug)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Subcommands {
    Install(Install),
    Update(Update),
    Login(Login),
    Push(Push),
    VersionDryRun(VersionDryRun),
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Install a dependency from soldeer repository or from a custom url that points to a zip file.\nExample: soldeer install @openzeppelin-contracts~2.3.0 the `~` is very important to differentiate between the name and the version that needs to be installed.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer install <DEPENDENCY>~<VERSION> [URL]"
)]
pub struct Install {
    #[clap(required = false)]
    pub dependency: Option<String>,
    #[clap(required = false)]
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Update dependencies by reading the config file.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer update"
)]
pub struct Update {}

#[derive(Debug, Clone, Parser)]
pub struct VersionDryRun {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Login into the central repository to push the dependencies.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer login"
)]
pub struct Login {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Push a dependency to the repository. The PATH_TO_DEPENDENCY is optional and if not provided, the current directory will be used.\nExample: If the directory is /home/soldeer/my_project and you do not specify the PATH_TO_DEPENDENCY,\nthe files inside the /home/soldeer/my_project will be pushed to the repository.\nIf you specify the PATH_TO_DEPENDENCY, the files inside the specified directory will be pushed to the repository.\nIf you want to ignore certain files, you can create a .soldeerignore file in the root of the project and add the files you want to ignore.\nThe .soldeerignore works like .gitignore.\nFor dry-run please use the --dry-run argument set to true, `soldeer push ... --dry-run true`. This will create a zip file that you can inspect and see what it will be pushed to the central repository.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer push <DEPENDENCY>~<VERSION> [PATH_TO_DEPENDENCY]"
)]
pub struct Push {
    #[clap(required = true)]
    pub dependency: String,
    pub path: Option<String>,
    #[arg(short, long)]
    pub dry_run: Option<bool>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    pub skip_warnings: Option<bool>,
}
