use crate::{
    config::{read_config_deps, Dependency, Paths, SoldeerConfig},
    errors::RemappingsError,
};
use path_slash::PathExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write as _,
};
use toml_edit::{value, Array, DocumentMut};

pub type Result<T> = std::result::Result<T, RemappingsError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RemappingsAction {
    Add(Dependency),
    Remove(Dependency),
    Update,
}

/// Location where to store the remappings, either in `remappings.txt` or the config file
/// (foundry/soldeer)
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RemappingsLocation {
    #[default]
    Txt,
    Config,
}

pub fn remappings_txt(
    action: &RemappingsAction,
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
    let existing_remappings: Vec<_> = contents.lines().filter_map(|r| r.split_once('=')).collect();

    let new_remappings = generate_remappings(action, paths, soldeer_config, &existing_remappings)?;

    let mut file = File::create(&paths.remappings)?;
    for remapping in new_remappings {
        writeln!(file, "{remapping}")?;
    }
    Ok(())
}

pub fn remappings_foundry(
    action: &RemappingsAction,
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
                let new_remappings = generate_remappings(action, paths, soldeer_config, &[])?;
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
            generate_remappings(action, paths, soldeer_config, &existing_remappings)?;
        remappings.clear();
        for remapping in new_remappings {
            remappings.push(remapping);
        }
    }

    fs::write(&paths.config, doc.to_string())?;
    Ok(())
}

pub fn edit_remappings(
    action: &RemappingsAction,
    config: &SoldeerConfig,
    paths: &Paths,
) -> Result<()> {
    if config.remappings_generate {
        if paths.config.to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(action, paths, config)?;
                }
                RemappingsLocation::Config => {
                    remappings_foundry(action, paths, config)?;
                }
            }
        } else {
            remappings_txt(action, paths, config)?;
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
    action: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
    existing_remappings: &[(&str, &str)],
) -> Result<Vec<String>> {
    let mut new_remappings = Vec::new();
    if soldeer_config.remappings_regenerate {
        new_remappings = remappings_from_deps(paths, soldeer_config)?;
    } else {
        match &action {
            RemappingsAction::Remove(remove_dep) => {
                // only keep items not matching the dependency to remove
                if let Ok(remove_og) = get_install_dir_relative(remove_dep, paths) {
                    for (existing_remapped, existing_og) in existing_remappings {
                        // TODO: make the detection smarter, and match on any path where the version
                        // is semver-compatible too.
                        if !existing_og.trim_end_matches('/').starts_with(&remove_og) {
                            new_remappings.push(format!("{existing_remapped}={existing_og}"));
                        }
                    }
                } else {
                    for (remapped, og) in existing_remappings {
                        new_remappings.push(format!("{remapped}={og}"));
                    }
                }
            }
            RemappingsAction::Add(add_dep) => {
                // we only add the remapping if it's not already existing, otherwise we keep the old
                // remapping
                let add_dep_remapped = format_remap_name(soldeer_config, add_dep);
                let add_dep_og = get_install_dir_relative(add_dep, paths)?;
                let mut found = false; // whether a remapping existed for that dep already
                for (existing_remapped, existing_og) in existing_remappings {
                    new_remappings.push(format!("{existing_remapped}={existing_og}"));
                    if existing_og.trim_end_matches('/').starts_with(&add_dep_og) {
                        found = true;
                    }
                }
                if !found {
                    new_remappings.push(format!("{add_dep_remapped}={add_dep_og}/"));
                }
            }
            RemappingsAction::Update => {
                // This is where we end up in the `update` command if we don't want to re-generate
                // all remappings. We need to merge existing remappings with the full list of deps.
                // We generate all remappings from the dependencies, then replace existing items.
                new_remappings = remappings_from_deps(paths, soldeer_config)?;
                if !existing_remappings.is_empty() {
                    for item in &mut new_remappings {
                        let (_, item_og) =
                            item.split_once('=').expect("remappings should have two parts");
                        // Try to find an existing item with the same path.
                        // TODO: make the detection smarter, and match on any path where the version
                        // is semver-compatible too.
                        // For this we need a reference to the dependency object so we can parse the
                        // version req string.
                        // If we found an existing remapping with a matching version, then we do a
                        // search and replace in the right-side (og) part to
                        // update the path to point to the new version
                        // folder. It's important to trim the trailing slash in case the existing
                        // remapping doesn't contain one.
                        if let Some((existing_remapped, existing_og)) =
                            existing_remappings.iter().find(|(_, og)| {
                                // if the existing remapping path starts with the dependency folder,
                                // we found a match
                                og.trim_end_matches('/').starts_with(item_og.trim_end_matches('/'))
                            })
                        {
                            // if found, we restore it
                            *item = format!("{existing_remapped}={existing_og}");
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

/// Find the install path (relative to project root) for a dependency that was already installed
///
/// # Errors
/// If the there is no folder in the dependencies folder corresponding to the dependency
fn get_install_dir_relative(dependency: &Dependency, paths: &Paths) -> Result<String> {
    let path = dunce::canonicalize(
        dependency
            .install_path_sync(&paths.dependencies)
            .ok_or(RemappingsError::DependencyNotFound(dependency.to_string()))?,
    )?;
    Ok(path
        .strip_prefix(&paths.root) // already canonicalized
        .map_err(|_| RemappingsError::DependencyNotFound(dependency.to_string()))?
        .to_slash_lossy()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GitDependency, HttpDependency};
    use testdir::testdir;

    #[test]
    fn test_get_install_dir_relative() {
        let dir = testdir!();
        fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
        let dependencies_dir = dir.join("dependencies");
        fs::create_dir_all(&dependencies_dir).unwrap();
        let paths = Paths::from_root(&dir).unwrap();

        fs::create_dir_all(dependencies_dir.join("dep1-1.1.1")).unwrap();
        let dependency =
            HttpDependency::builder().name("dep1").version_req("^1.0.0").build().into();
        let res = get_install_dir_relative(&dependency, &paths);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "dependencies/dep1-1.1.1");

        fs::create_dir_all(dependencies_dir.join("dep2-2.0.0")).unwrap();
        let dependency = GitDependency::builder()
            .name("dep2")
            .version_req("2.0.0")
            .git("git@github.com:test/test.git")
            .build()
            .into();
        let res = get_install_dir_relative(&dependency, &paths);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "dependencies/dep2-2.0.0");

        let dependency = HttpDependency::builder().name("dep3").version_req("3.0.0").build().into();
        let res = get_install_dir_relative(&dependency, &paths);
        assert!(res.is_err(), "{res:?}");
    }

    #[test]
    fn test_format_remap_name() {
        let dependency =
            HttpDependency::builder().name("dep1").version_req("^1.0.0").build().into();
        let res = format_remap_name(
            &SoldeerConfig {
                remappings_version: false,
                remappings_prefix: String::new(),
                ..Default::default()
            },
            &dependency,
        );
        assert_eq!(res, "dep1/");
        let res = format_remap_name(
            &SoldeerConfig {
                remappings_version: true,
                remappings_prefix: String::new(),
                ..Default::default()
            },
            &dependency,
        );
        assert_eq!(res, "dep1-^1.0.0/");
        let res = format_remap_name(
            &SoldeerConfig {
                remappings_version: false,
                remappings_prefix: "@".to_string(),
                ..Default::default()
            },
            &dependency,
        );
        assert_eq!(res, "@dep1/");
        let res = format_remap_name(
            &SoldeerConfig {
                remappings_version: true,
                remappings_prefix: "@".to_string(),
                ..Default::default()
            },
            &dependency,
        );
        assert_eq!(res, "@dep1-^1.0.0/");

        let dependency =
            HttpDependency::builder().name("dep1").version_req("=1.0.0").build().into();
        let res = format_remap_name(
            &SoldeerConfig {
                remappings_version: true,
                remappings_prefix: String::new(),
                ..Default::default()
            },
            &dependency,
        );
        assert_eq!(res, "dep1-1.0.0/");
    }

    #[test]
    fn test_remappings_from_deps() {
        let dir = testdir!();
        let config = r#"[dependencies]
dep1 = "^1.0.0"
dep2 = "2.0.0"
dep3 = { version = "foobar", git = "git@github.com:test/test.git", branch = "foobar" }
"#;
        fs::write(dir.join("soldeer.toml"), config).unwrap();
        let dependencies_dir = dir.join("dependencies");
        fs::create_dir_all(&dependencies_dir).unwrap();
        let paths = Paths::from_root(&dir).unwrap();

        fs::create_dir_all(dependencies_dir.join("dep1-1.1.1")).unwrap();
        fs::create_dir_all(dependencies_dir.join("dep2-2.0.0")).unwrap();
        fs::create_dir_all(dependencies_dir.join("dep3-foobar")).unwrap();

        let res = remappings_from_deps(&paths, &SoldeerConfig::default());
        assert!(res.is_ok(), "{res:?}");
        let res = res.unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], "dep1-^1.0.0/=dependencies/dep1-1.1.1/");
        assert_eq!(res[1], "dep2-2.0.0/=dependencies/dep2-2.0.0/");
        assert_eq!(res[2], "dep3-foobar/=dependencies/dep3-foobar/");
    }

    #[test]
    fn test_generate_remappings_add() {
        let dir = testdir!();
        fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let config = SoldeerConfig::default();
        // empty existing remappings
        let existing_deps = vec![];
        let dep = HttpDependency::builder().name("lib1").version_req("1.0.0").build().into();
        let res = generate_remappings(&RemappingsAction::Add(dep), &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/"]);

        // existing remappings not matching new one
        let existing_deps = vec![("lib1-1.0.0/", "dependencies/lib1-1.0.0/")];
        fs::create_dir_all(paths.dependencies.join("lib2-1.1.1")).unwrap();
        let dep = HttpDependency::builder().name("lib2").version_req("^1.0.0").build().into();
        let res = generate_remappings(&RemappingsAction::Add(dep), &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/", "lib2-^1.0.0/=dependencies/lib2-1.1.1/"]
        );

        // existing remappings matching the new one
        let existing_deps = vec![("@lib1-1.0.0/foo", "dependencies/lib1-1.0.0/src")];
        let dep = HttpDependency::builder().name("lib1").version_req("1.0.0").build().into();
        let res = generate_remappings(&RemappingsAction::Add(dep), &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), vec!["@lib1-1.0.0/foo=dependencies/lib1-1.0.0/src"]);
    }

    #[test]
    fn test_generate_remappings_remove() {
        let dir = testdir!();
        fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib2-2.0.0")).unwrap();
        let config = SoldeerConfig::default();
        let existing_deps = vec![
            ("lib1-1.0.0/", "dependencies/lib1-1.0.0/"),
            ("lib2-2.0.0/", "dependencies/lib2-2.0.0/"),
        ];
        let dep = HttpDependency::builder().name("lib1").version_req("1.0.0").build().into();
        let res =
            generate_remappings(&RemappingsAction::Remove(dep), &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), vec!["lib2-2.0.0/=dependencies/lib2-2.0.0/"]);

        // dep does not exist, no error
        let dep = HttpDependency::builder().name("lib3").version_req("1.0.0").build().into();
        let res =
            generate_remappings(&RemappingsAction::Remove(dep), &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/", "lib2-2.0.0/=dependencies/lib2-2.0.0/"]
        );
    }

    #[test]
    fn test_generate_remappings_update() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
lib2 = "2.0.0"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib2-2.0.0")).unwrap();
        let config = SoldeerConfig::default();
        // all entries are customized
        let existing_deps = vec![
            ("lib1-1.0.0/", "dependencies/lib1-1.0.0/src/"),
            ("lib2/", "dependencies/lib2-2.0.0/"),
        ];
        let res = generate_remappings(&RemappingsAction::Update, &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/src/", "lib2/=dependencies/lib2-2.0.0/"]
        );

        // one entry is missing
        let existing_deps = vec![("lib1-1.0.0/", "dependencies/lib1-1.0.0/")];
        let res = generate_remappings(&RemappingsAction::Update, &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/", "lib2-2.0.0/=dependencies/lib2-2.0.0/"]
        );

        // extra entries are removed
        let existing_deps = vec![
            ("lib1-1.0.0/", "dependencies/lib1-1.0.0/"),
            ("lib2-2.0.0/", "dependencies/lib2-2.0.0/"),
            ("lib3/", "dependencies/lib3/"),
        ];
        let res = generate_remappings(&RemappingsAction::Update, &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/", "lib2-2.0.0/=dependencies/lib2-2.0.0/"]
        );
    }

    #[test]
    fn test_remappings_foundry_noprofile() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        let config = SoldeerConfig::default();
        // no profile: no remappings are added
        let res = remappings_foundry(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), contents);
    }

    #[test]
    fn test_remappings_foundry_default_profile_empty() {
        let dir = testdir!();
        let contents = r#"[profile.default]

[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let config = SoldeerConfig::default();
        let res = remappings_foundry(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.config).unwrap();
        let doc: DocumentMut = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["profile"]["default"]["remappings"]
                .as_array()
                .unwrap()
                .into_iter()
                .map(|i| i.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/"]
        );
    }

    #[test]
    fn test_remappings_foundry_second_profile_empty() {
        let dir = testdir!();
        let contents = r#"[profile.default]

[profile.local]

[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let config = SoldeerConfig::default();
        // should only add remappings to the default profile
        let res = remappings_foundry(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.config).unwrap();
        let doc: DocumentMut = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["profile"]["default"]["remappings"]
                .as_array()
                .unwrap()
                .into_iter()
                .map(|i| i.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/"]
        );
        assert!(!doc["profile"]["local"].as_table().unwrap().contains_key("remappings"));
    }

    #[test]
    fn test_remappings_foundry_two_profiles() {
        let dir = testdir!();
        let contents = r#"[profile.default]
remappings = []

[profile.local]
remappings = []

[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let config = SoldeerConfig::default();
        let res = remappings_foundry(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.config).unwrap();
        let doc: DocumentMut = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["profile"]["default"]["remappings"]
                .as_array()
                .unwrap()
                .into_iter()
                .map(|i| i.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/"]
        );
        assert_eq!(
            doc["profile"]["local"]["remappings"]
                .as_array()
                .unwrap()
                .into_iter()
                .map(|i| i.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["lib1-1.0.0/=dependencies/lib1-1.0.0/"]
        );
    }

    #[test]
    fn test_remappings_foundry_keep_existing() {
        let dir = testdir!();
        let contents = r#"[profile.default]
remappings = ["lib1/=dependencies/lib1-1.0.0/src/"]

[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let config = SoldeerConfig::default();
        let res = remappings_foundry(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.config).unwrap();
        let doc: DocumentMut = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(
            doc["profile"]["default"]["remappings"]
                .as_array()
                .unwrap()
                .into_iter()
                .map(|i| i.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["lib1/=dependencies/lib1-1.0.0/src/"]
        );
    }

    #[test]
    fn test_remappings_txt_keep() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let remappings = "lib1/=dependencies/lib1-1.0.0/src/\n";
        fs::write(dir.join("remappings.txt"), remappings).unwrap();
        let config = SoldeerConfig::default();
        let res = remappings_txt(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.remappings).unwrap();
        assert_eq!(contents, remappings);
    }

    #[test]
    fn test_remappings_txt_regenerate() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        let remappings = "lib1/=dependencies/lib1-1.0.0/src/\n";
        fs::write(dir.join("remappings.txt"), remappings).unwrap();
        let config = SoldeerConfig { remappings_regenerate: true, ..Default::default() };
        let res = remappings_txt(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.remappings).unwrap();
        assert_eq!(contents, "lib1-1.0.0/=dependencies/lib1-1.0.0/\n");
    }

    #[test]
    fn test_remappings_txt_missing() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
lib2 = "2.0.0"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib2-2.0.0")).unwrap();
        let remappings = "lib1/=dependencies/lib1-1.0.0/src/\n";
        fs::write(dir.join("remappings.txt"), remappings).unwrap();
        let config = SoldeerConfig::default();
        let res = remappings_txt(&RemappingsAction::Update, &paths, &config);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.remappings).unwrap();
        assert_eq!(
            contents,
            "lib1/=dependencies/lib1-1.0.0/src/\nlib2-2.0.0/=dependencies/lib2-2.0.0/\n"
        );
    }
}
