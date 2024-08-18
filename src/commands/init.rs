use super::Result;
use crate::{
    config::{add_to_config, get_config_path, read_soldeer_config, remove_forge_lib},
    install::{add_to_remappings, ensure_dependencies_dir, install_dependency, Progress},
    lock::add_to_lockfile,
    remote::get_latest_forge_std,
    SoldeerError,
};
use cliclack::{
    log::{remark, success},
    multi_progress,
};

pub(crate) async fn init_command(cmd: super::Init) -> Result<()> {
    if cmd.clean {
        remark("Flag `--clean` was set, removing `lib` dir and submodules")?;
        remove_forge_lib().await?;
    }

    let config_path = get_config_path()?;
    let config = read_soldeer_config(Some(&config_path))?;
    success("Done reading config")?;
    let dependency = get_latest_forge_std()
        .await
        .map_err(|e| SoldeerError::DownloadError { dep: "forge-std".to_string(), source: e })?;
    ensure_dependencies_dir()?;
    let multi = multi_progress(format!("Installing {dependency}"));
    let progress = Progress::new(&multi, 1);
    progress.start_all();
    let lock =
        install_dependency(&dependency, None, false, progress.clone()).await.map_err(|e| {
            multi.error(&e);
            e
        })?;
    progress.stop_all();
    multi.stop();
    add_to_config(&dependency, &config_path)?;
    success("Dependency added to config")?;
    add_to_lockfile(lock)?;
    success("Dependency added to lockfile")?;
    add_to_remappings(dependency, &config, &config_path).await?;
    success("Dependency added to remappings")?;
    // TODO: add `dependencies` to the .gitignore file if it exists
    Ok(())
}
