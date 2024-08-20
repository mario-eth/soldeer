use crate::{
    config::{read_config_deps, Dependency, SoldeerConfig},
    errors::RemappingsError,
    utils::sanitize_filename,
    REMAPPINGS_FILE,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write as _,
    path::Path,
};
use toml_edit::{value, Array, DocumentMut};

pub type Result<T> = std::result::Result<T, RemappingsError>;

#[derive(Debug, Clone, PartialEq)]
pub enum RemappingsAction {
    Add(Dependency),
    Remove(Dependency),
    None,
}

/// Location where to store the remappings, either in `remappings.txt` or the config file
/// (foundry/soldeer)
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RemappingsLocation {
    #[default]
    Txt,
    Config,
}

pub async fn remappings_txt(
    dependency: &RemappingsAction,
    config_path: impl AsRef<Path>,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    if soldeer_config.remappings_regenerate && REMAPPINGS_FILE.exists() {
        fs::remove_file(REMAPPINGS_FILE.as_path())?;
    }
    let contents = match REMAPPINGS_FILE.exists() {
        true => fs::read_to_string(REMAPPINGS_FILE.as_path())?,
        false => "".to_string(),
    };
    let existing_remappings = contents.lines().filter_map(|r| r.split_once('=')).collect();

    let new_remappings =
        generate_remappings(dependency, config_path, soldeer_config, existing_remappings)?;

    let mut file = File::create(REMAPPINGS_FILE.as_path())?;
    for remapping in new_remappings {
        writeln!(file, "{}", remapping)?;
    }
    Ok(())
}

pub async fn remappings_foundry(
    dependency: &RemappingsAction,
    config_path: impl AsRef<Path>,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    let contents = fs::read_to_string(&config_path)?;
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    let Some(profiles) = doc["profile"].as_table_mut() else {
        // we don't add remappings if there are no profiles
        return Ok(());
    };

    for (name, profile) in profiles.iter_mut() {
        // we normally only edit remappings of profiles which already have a remappings key
        let Some(Some(remappings)) = profile.get_mut("remappings").map(|v| v.as_array_mut()) else {
            // except the default profile, where we always add the remappings
            if name == "default" {
                let new_remappings =
                    generate_remappings(dependency, &config_path, soldeer_config, vec![])?;
                let array = Array::from_iter(new_remappings.into_iter());
                profile["remappings"] = value(array);
            }
            continue;
        };
        let existing_remappings: Vec<_> = remappings
            .iter()
            .filter_map(|r| r.as_str())
            .filter_map(|r| r.split_once('='))
            .collect();
        let new_remappings =
            generate_remappings(dependency, &config_path, soldeer_config, existing_remappings)?;
        remappings.clear();
        for remapping in new_remappings {
            remappings.push(remapping);
        }
    }

    fs::write(config_path, doc.to_string())?;
    Ok(())
}

pub async fn add_to_remappings(
    dep: Dependency,
    config: &SoldeerConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    if config.remappings_generate {
        if config_path.as_ref().to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::Add(dep), &config_path, config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::Add(dep), &config_path, config).await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::Add(dep), &config_path, config).await?;
        }
    }
    Ok(())
}

pub async fn remove_from_remappings(
    dep: Dependency,
    config: &SoldeerConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    if config.remappings_generate {
        if config_path.as_ref().to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::Remove(dep), &config_path, &config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::Remove(dep), &config_path, &config)
                        .await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::Remove(dep), &config_path, &config).await?;
        }
    }
    Ok(())
}

pub async fn update_remappings(
    config: &SoldeerConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    if config.remappings_generate {
        if config_path.as_ref().to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::None, &config_path, config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::None, &config_path, config).await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::None, &config_path, config).await?;
        }
    }
    Ok(())
}

fn generate_remappings(
    dependency: &RemappingsAction,
    config_path: impl AsRef<Path>,
    soldeer_config: &SoldeerConfig,
    existing_remappings: Vec<(&str, &str)>,
) -> Result<Vec<String>> {
    let mut new_remappings = Vec::new();
    if soldeer_config.remappings_regenerate {
        new_remappings = remappings_from_deps(config_path, soldeer_config)?;
    } else {
        match &dependency {
            RemappingsAction::Remove(remove_dep) => {
                // only keep items not matching the dependency to remove
                let sanitized_name =
                    sanitize_filename(&format!("{}-{}", remove_dep.name(), remove_dep.version()));
                let remove_dep_orig = format!("dependencies/{sanitized_name}/");
                for (remapped, orig) in existing_remappings {
                    if orig != remove_dep_orig {
                        new_remappings.push(format!("{}={}", remapped, orig));
                    }
                }
            }
            RemappingsAction::Add(add_dep) => {
                // we only add the remapping if it's not already existing, otherwise we keep the old
                // remapping
                let new_dep_remapped = format_remap_name(soldeer_config, add_dep);
                let sanitized_name =
                    sanitize_filename(&format!("{}-{}", add_dep.name(), add_dep.version()));
                let new_dep_orig = format!("dependencies/{}/", sanitized_name);
                let mut found = false; // whether a remapping existed for that dep already
                for (remapped, orig) in existing_remappings {
                    new_remappings.push(format!("{}={}", remapped, orig));
                    if orig == new_dep_orig {
                        found = true;
                    }
                }
                if !found {
                    new_remappings.push(format!("{}={}", new_dep_remapped, new_dep_orig));
                }
            }
            RemappingsAction::None => {
                // This is where we end up in the `update` command if we don't want to re-generate
                // all remappings. We need to merge existing remappings with the full list of deps.
                // We generate all remappings from the dependencies, then replace existing items.
                new_remappings = remappings_from_deps(config_path, soldeer_config)?;
                if !existing_remappings.is_empty() {
                    for item in new_remappings.iter_mut() {
                        let (_, item_orig) =
                            item.split_once('=').expect("remappings should have two parts");
                        // try to find an existing item with the same path
                        if let Some((remapped, orig)) =
                            existing_remappings.iter().find(|(_, o)| item_orig == *o)
                        {
                            *item = format!("{}={}", remapped, orig);
                        }
                    }
                }
            }
        }
    }

    // sort the remappings
    new_remappings.sort_unstable();
    Ok(new_remappings)
}

fn remappings_from_deps(
    config_path: impl AsRef<Path>,
    soldeer_config: &SoldeerConfig,
) -> Result<Vec<String>> {
    let config_path = config_path.as_ref().to_path_buf();
    let dependencies = read_config_deps(Some(config_path))?;
    Ok(dependencies
        .iter()
        .map(|dependency| {
            let dependency_name_formatted = format_remap_name(soldeer_config, dependency);
            format!(
                "{dependency_name_formatted}=dependencies/{}-{}/",
                dependency.name(),
                dependency.version()
            )
        })
        .collect())
}

fn format_remap_name(soldeer_config: &SoldeerConfig, dependency: &Dependency) -> String {
    let version_suffix =
        if soldeer_config.remappings_version { &format!("-{}", dependency.version()) } else { "" };
    format!("{}{}{}/", soldeer_config.remappings_prefix, dependency.name(), version_suffix)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fs::remove_file;
    use rand::{distributions::Alphanumeric, Rng as _};
    use serial_test::serial;

    use crate::{
        config::{read_soldeer_config, HttpDependency},
        utils::{get_current_working_dir, read_file_to_string},
    };

    use super::*;

    #[tokio::test]
    async fn generate_remappings_with_prefix_and_version_in_config() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[dependencies]
[soldeer]
remappings_prefix = "@"
remappings_version = true
remappings_location = "config"
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ =
            remappings_foundry(&RemappingsAction::Add(dependency), &target_config, &soldeer_config)
                .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["@dep1-1.0.0/=dependencies/dep1-1.0.0/"]
[dependencies]
[soldeer]
remappings_prefix = "@"
remappings_version = true
remappings_location = "config"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn generate_remappings_no_prefix_and_no_version_in_config() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[dependencies]
[soldeer]
remappings_generate = true
remappings_version = false
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::Add(dependency),
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep1/=dependencies/dep1-1.0.0/"]
[dependencies]
[soldeer]
remappings_generate = true
remappings_version = false
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn generate_remappings_prefix_and_version_in_txt() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[dependencies]
[soldeer]
remappings_generate = true
remappings_prefix = "@"
remappings_version = true
"#;

        let target_config = define_config(true);
        let txt = get_current_working_dir().join("remappings.txt");
        let _ = remove_file(&txt);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_txt(&RemappingsAction::Add(dependency), &target_config, &soldeer_config)
            .await;

        content = "@dep1-1.0.0/=dependencies/dep1-1.0.0/\n";

        assert_eq!(read_file_to_string(&txt), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn generate_remappings_no_prefix_and_no_version_in_txt() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[dependencies]
[soldeer]
remappings_generate = true
remappings_version = false
"#;

        let target_config = define_config(true);
        let txt = get_current_working_dir().join("remappings.txt");
        let _ = remove_file(&txt);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_txt(&RemappingsAction::Add(dependency), &target_config, &soldeer_config)
            .await;

        content = "dep1/=dependencies/dep1-1.0.0/\n";

        assert_eq!(read_file_to_string(&txt), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn generate_remappings_in_config_only_default_profile() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[profile.local.testing]
ffi = true
[dependencies]
[soldeer]
remappings_generate = true
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::Add(dependency),
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep1-1.0.0/=dependencies/dep1-1.0.0/"]
[profile.local.testing]
ffi = true
[dependencies]
[soldeer]
remappings_generate = true
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn generate_remappings_in_config_all_profiles() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
[profile.local]
remappings = []
[profile.local.testing]
ffi = true
[dependencies]
[soldeer]
remappings_generate = true
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::Add(dependency),
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep1-1.0.0/=dependencies/dep1-1.0.0/"]
[profile.local]
remappings = ["dep1-1.0.0/=dependencies/dep1-1.0.0/"]
[profile.local.testing]
ffi = true
[dependencies]
[soldeer]
remappings_generate = true
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn generate_remappings_in_config_existing() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep2-1.0.0/=dependencies/dep2-1.0.0/"]
[dependencies]
[soldeer]
remappings_generate = true
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        });
        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::Add(dependency),
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep1-1.0.0/=dependencies/dep1-1.0.0/", "dep2-1.0.0/=dependencies/dep2-1.0.0/"]
[dependencies]
[soldeer]
remappings_generate = true
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn generate_remappings_regenerate() -> Result<()> {
        let mut content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["@dep2-custom/=dependencies/dep2-1.0.0/"]
[dependencies]
dep2 = "1.0.0"
[soldeer]
remappings_generate = true
remappings_regenerate = true
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::None,
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["dep2-1.0.0/=dependencies/dep2-1.0.0/"]
[dependencies]
dep2 = "1.0.0"
[soldeer]
remappings_generate = true
remappings_regenerate = true
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn generate_remappings_keep_custom() -> Result<()> {
        let content = r#"
[profile.default]
solc = "0.8.26"
libs = ["dependencies"]
remappings = ["@dep2-custom/=dependencies/dep2-1.0.0/"]
[dependencies]
dep2 = "1.0.0"
[soldeer]
remappings_generate = true
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
        let _ = remappings_foundry(
            &RemappingsAction::None,
            target_config.to_str().unwrap(),
            &soldeer_config,
        )
        .await;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    ////////////// UTILS //////////////

    fn write_to_config(target_file: &PathBuf, content: &str) {
        if target_file.exists() {
            let _ = remove_file(target_file);
        }
        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(target_file).unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn define_config(foundry: bool) -> PathBuf {
        let s: String =
            rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
        let mut target = format!("foundry{}.toml", s);
        if !foundry {
            target = format!("soldeer{}.toml", s);
        }

        get_current_working_dir().join("test").join(target)
    }
}
