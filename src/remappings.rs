use crate::{
    config::{read_config_deps, Dependency, Paths, SoldeerConfig},
    errors::RemappingsError,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write as _,
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

pub fn remappings_txt(
    dependency: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    if soldeer_config.remappings_regenerate && paths.remappings.exists() {
        fs::remove_file(&paths.remappings)?;
    }
    let contents = if paths.remappings.exists() {
        fs::read_to_string(&paths.remappings)?
    } else {
        String::new()
    };
    let existing_remappings = contents.lines().filter_map(|r| r.split_once('=')).collect();

    let new_remappings =
        generate_remappings(dependency, paths, soldeer_config, existing_remappings)?;

    let mut file = File::create(&paths.remappings)?;
    for remapping in new_remappings {
        writeln!(file, "{remapping}")?;
    }
    Ok(())
}

pub fn remappings_foundry(
    dependency: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    let contents = fs::read_to_string(&paths.config)?;
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
                    generate_remappings(dependency, paths, soldeer_config, vec![])?;
                let array = new_remappings.into_iter().collect::<Array>();
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
            generate_remappings(dependency, paths, soldeer_config, existing_remappings)?;
        remappings.clear();
        for remapping in new_remappings {
            remappings.push(remapping);
        }
    }

    fs::write(&paths.config, doc.to_string())?;
    Ok(())
}

pub fn edit_remappings(
    dep: &RemappingsAction,
    config: &SoldeerConfig,
    paths: &Paths,
) -> Result<()> {
    if config.remappings_generate {
        if paths.config.to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(dep, paths, config)?;
                }
                RemappingsLocation::Config => {
                    remappings_foundry(dep, paths, config)?;
                }
            }
        } else {
            remappings_txt(dep, paths, config)?;
        }
    }
    Ok(())
}

pub fn format_remap_name(soldeer_config: &SoldeerConfig, dependency: &Dependency) -> String {
    let version_suffix = if soldeer_config.remappings_version {
        &format!("-{}", dependency.version_req().replace('=', ""))
    } else {
        ""
    };
    format!("{}{}{}/", soldeer_config.remappings_prefix, dependency.name(), version_suffix)
}

fn generate_remappings(
    dependency: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
    existing_remappings: Vec<(&str, &str)>,
) -> Result<Vec<String>> {
    let mut new_remappings = Vec::new();
    if soldeer_config.remappings_regenerate {
        new_remappings = remappings_from_deps(paths, soldeer_config)?;
    } else {
        match &dependency {
            RemappingsAction::Remove(remove_dep) => {
                // only keep items not matching the dependency to remove
                let remove_remapped = format_remap_name(soldeer_config, remove_dep);
                for (remapped, orig) in existing_remappings {
                    if remapped != remove_remapped {
                        new_remappings.push(format!("{remapped}={orig}"));
                    }
                }
            }
            RemappingsAction::Add(add_dep) => {
                // we only add the remapping if it's not already existing, otherwise we keep the old
                // remapping
                let new_dep_remapped = format_remap_name(soldeer_config, add_dep);
                let new_dep_path = get_install_dir_relative(add_dep, paths)?;
                let mut found = false; // whether a remapping existed for that dep already
                for (remapped, orig) in existing_remappings {
                    new_remappings.push(format!("{remapped}={orig}"));
                    if remapped == new_dep_remapped {
                        found = true;
                    }
                }
                if !found {
                    new_remappings.push(format!("{new_dep_remapped}={new_dep_path}"));
                }
            }
            RemappingsAction::None => {
                // This is where we end up in the `update` command if we don't want to re-generate
                // all remappings. We need to merge existing remappings with the full list of deps.
                // We generate all remappings from the dependencies, then replace existing items.
                new_remappings = remappings_from_deps(paths, soldeer_config)?;
                if !existing_remappings.is_empty() {
                    for item in &mut new_remappings {
                        let (item_remapped, _) =
                            item.split_once('=').expect("remappings should have two parts");
                        // try to find an existing item with the same name
                        if let Some((remapped, orig)) =
                            existing_remappings.iter().find(|(r, _)| item_remapped == *r)
                        {
                            *item = format!("{remapped}={orig}");
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

fn remappings_from_deps(paths: &Paths, soldeer_config: &SoldeerConfig) -> Result<Vec<String>> {
    let dependencies = read_config_deps(&paths.config)?;
    dependencies
        .iter()
        .map(|dependency| {
            let dependency_name_formatted = format_remap_name(soldeer_config, dependency);
            let relative_path = get_install_dir_relative(dependency, paths)?;
            Ok(format!("{dependency_name_formatted}={relative_path}/"))
        })
        .collect::<Result<Vec<_>>>()
}

fn get_install_dir_relative(dependency: &Dependency, paths: &Paths) -> Result<String> {
    let path = dependency
        .install_path_sync(&paths.dependencies)
        .ok_or(RemappingsError::DependencyNotFound(dependency.to_string()))?
        .canonicalize()?;
    Ok(path
        .strip_prefix(&paths.root.canonicalize()?)
        .map_err(|_| RemappingsError::DependencyNotFound(dependency.to_string()))?
        .to_string_lossy()
        .to_string())
}

#[cfg(test)]
mod tests {
    /* use std::path::PathBuf;

        use fs::remove_file;
        use rand::{distributions::Alphanumeric, Rng as _};
        use serial_test::serial;

        use crate::config::{read_soldeer_config, HttpDependency};

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
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ =
                remappings_foundry(&RemappingsAction::Add(dependency), &target_config, &soldeer_config);

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_foundry(
                &RemappingsAction::Add(dependency),
                target_config.to_str().unwrap(),
                &soldeer_config,
            );

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
            let txt = REMAPPINGS_FILE.to_path_buf();
            let _ = remove_file(&txt);

            write_to_config(&target_config, content);
            let dependency = Dependency::Http(HttpDependency {
                name: "dep1".to_string(),
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_txt(&RemappingsAction::Add(dependency), &target_config, &soldeer_config);

            content = "@dep1-1.0.0/=dependencies/dep1-1.0.0/\n";

            assert_eq!(fs::read_to_string(&txt).unwrap(), content);

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
            let txt = REMAPPINGS_FILE.to_path_buf();
            let _ = remove_file(&txt);

            write_to_config(&target_config, content);
            let dependency = Dependency::Http(HttpDependency {
                name: "dep1".to_string(),
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_txt(&RemappingsAction::Add(dependency), &target_config, &soldeer_config);

            content = "dep1/=dependencies/dep1-1.0.0/\n";

            assert_eq!(fs::read_to_string(&txt).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_foundry(
                &RemappingsAction::Add(dependency),
                target_config.to_str().unwrap(),
                &soldeer_config,
            );

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_foundry(
                &RemappingsAction::Add(dependency),
                target_config.to_str().unwrap(),
                &soldeer_config,
            );

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: None,
            });
            let soldeer_config = read_soldeer_config(Some(target_config.clone())).unwrap();
            let _ = remappings_foundry(
                &RemappingsAction::Add(dependency),
                target_config.to_str().unwrap(),
                &soldeer_config,
            );

            content = r#"
    [profile.default]
    solc = "0.8.26"
    libs = ["dependencies"]
    remappings = ["dep1-1.0.0/=dependencies/dep1-1.0.0/", "dep2-1.0.0/=dependencies/dep2-1.0.0/"]
    [dependencies]
    [soldeer]
    remappings_generate = true
    "#;

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
            );

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
            );

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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

            PROJECT_ROOT.join("test").join(target)
        } */
}
