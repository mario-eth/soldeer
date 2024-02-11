use clap::{
    Parser,
    Subcommand,
};

#[derive(Parser, Debug)]
#[clap(
    name = "soldeer",
    author = "m4rio.eth",
    version,
    about = "A minimal solidity dependency manager"
)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    Install(Install),
    Update(Update),
    Login(Login),
    Push(Push),
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Install a dependency from soldeer repository or from a custom url that points to a zip file. Example: dependency~version. the `~` is very important to differentiate between the name and the version that needs to be installed.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer install <DEPENDENCY>~<VERSION> [URL]"
)]
pub struct Install {
    #[clap(required = true)]
    pub dependency: String,
    #[clap(required = false)]
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Update dependencies by reading the config file",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer update"
)]
pub struct Update {}

#[derive(Debug, Clone, Parser)]
pub struct Help {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Login into the central repository to push the dependencies.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer login"
)]
pub struct Login {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Push a dependency to the repository. The PATH_TO_DEPENDENCY is optional and if not provided, the current directory will be used. \nExample: If the directory is /home/soldeer/my_project and you do not specify the PATH_TO_DEPENDENCY, \nthe files inside the /home/soldeer/my_project will be pushed to the repository. \nIf you specify the PATH_TO_DEPENDENCY, the files inside the specified directory will be pushed to the repository. \nExample: soldeer push dependency~version /home/soldeer/my_project",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer push <DEPENDENCY>~<VERSION> [PATH_TO_DEPENDENCY]"
)]
pub struct Push {
    #[clap(required = true)]
    pub dependency: String,
    pub path: Option<String>,
}
