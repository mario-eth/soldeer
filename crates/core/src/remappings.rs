//! Remappings management.
use crate::{
    config::{read_config_deps, Dependency, Paths, SoldeerConfig},
    errors::RemappingsError,
    utils::path_matches,
};
use derive_more::derive::From;
use log::debug;
use path_slash::PathExt as _;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write as _,
    path::PathBuf,
};
use toml_edit::{value, Array, DocumentMut};

pub type Result<T> = std::result::Result<T, RemappingsError>;

/// Action to perform on the remappings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RemappingsAction {
    /// Add a dependency to the remappings.
    Add(Dependency),

    /// Remove a dependency from the remappings.
    Remove(Dependency),

    /// Update the remappings according to the config file.
    Update,
}

/// Location where to store the remappings, either in `remappings.txt` or the config file
/// (foundry/soldeer).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RemappingsLocation {
    /// Store the remappings in a dedicated `remappings.txt` file.
    #[default]
    Txt,

    /// Store the remappings in the `foundry.toml` config file.
    ///
    /// Note that remappings are never stored in the `soldeer.toml` file because foundry wouldn't
    /// be able to read them from there.
    Config,
}

/// Generate the remappings for storing into the `remappings.txt` file.
///
/// If the `remappings_regenerate` option is set to `true`, then any existing remappings are
/// discarded and the remappings are generated from the dependencies in the config file.
///
/// Otherwise, existing remappings are kept, and depending on the action, a remapping entry is added
/// or removed. For the [`RemappingsAction::Update`] action, the existing remappings are merged with
/// the dependencies in the config file.
pub fn remappings_txt(
    action: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    if soldeer_config.remappings_regenerate && paths.remappings.exists() {
        fs::remove_file(&paths.remappings)?;
        debug!(path:? = paths.remappings; "removed existing remappings file");
    }
    let contents = if paths.remappings.exists() {
        debug!(path:? = paths.remappings; "reading existing remappings from remappings.txt file");
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
    debug!(path:? = paths.remappings; "updated remappings.txt file");
    Ok(())
}

/// Generate the remappings for storing into the `foundry.toml` config file.
///
/// If the `remappings_regenerate` option is set to `true`, then any existing remappings are
/// discarded and the remappings are generated from the dependencies in the config file.
///
/// Otherwise, existing remappings are kept, and depending on the action, a remapping entry is added
/// or removed. For the [`RemappingsAction::Update`] action, the existing remappings are merged with
/// the dependencies in the config file.
///
/// The remappings are added to the default profile in all cases, and to any other profile that
/// already has a `remappings key`. If the profile doesn't have a remappings key, it is left
/// untouched.
pub fn remappings_foundry(
    action: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    let contents = fs::read_to_string(&paths.config)?;
    let mut doc: DocumentMut =
        contents.parse::<DocumentMut>().expect("config file should be valid toml");
    let Some(profiles) = doc["profile"].as_table_mut() else {
        // we don't add remappings if there are no profiles
        debug!("no config profile found, skipping remappings generation");
        return Ok(());
    };

    for (name, profile) in profiles.iter_mut() {
        // we normally only edit remappings of profiles which already have a remappings key
        match profile.get_mut("remappings").map(|v| v.as_array_mut()) {
            Some(Some(remappings)) => {
                debug!(name:% = name; "updating remappings for profile");
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
                format_array(remappings);
            }
            _ => {
                if name == "default" {
                    debug!("updating remappings for default profile");
                    // except the default profile, where we always add the remappings
                    let new_remappings = generate_remappings(action, paths, soldeer_config, &[])?;
                    let mut array = new_remappings.into_iter().collect::<Array>();
                    format_array(&mut array);
                    profile["remappings"] = value(array);
                }
            }
        }
    }

    fs::write(&paths.config, doc.to_string())?;
    debug!(path:? = paths.config; "remappings updated in config file");

    Ok(())
}

/// Edit the remappings according to the action and the configuration.
///
/// Depending on the configuration, the remappings are either stored in a `remappings.txt` file or
/// in the `foundry.toml` config file.
///
/// Note that if the config is stored in a dedicated `soldeer.toml` file, then the
/// `remappings_location` setting is ignored and the remappings are always stored in a
/// `remappings.txt` file.
pub fn edit_remappings(
    action: &RemappingsAction,
    config: &SoldeerConfig,
    paths: &Paths,
) -> Result<()> {
    if config.remappings_generate {
        if paths.config.to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    debug!("updating remappings.txt according to config option");
                    remappings_txt(action, paths, config)?;
                }
                RemappingsLocation::Config => {
                    debug!("updating foundry.toml remappings according to config option");
                    remappings_foundry(action, paths, config)?;
                }
            }
        } else {
            debug!("updating remappings.txt because config file is soldeer.toml");
            remappings_txt(action, paths, config)?;
        }
    } else {
        debug!("skipping remappings update according to config option");
    }
    Ok(())
}

/// Format the default left part (alias) for a remappings entry.
///
/// The optional `remappings_prefix` setting is prepended to the dependency name, and the
/// version requirement string is appended (after a hyphen) if the `remappings_version` setting is
/// set to `true`. Finally, a trailing slash is added to the alias.
pub fn format_remap_name(soldeer_config: &SoldeerConfig, dependency: &Dependency) -> String {
    let version_suffix = if soldeer_config.remappings_version {
        &format!("-{}", dependency.version_req().replace('=', ""))
    } else {
        ""
    };
    format!("{}{}{}/", soldeer_config.remappings_prefix, dependency.name(), version_suffix)
}

/// Generate the remappings for a given action.
///
/// If the `remappings_regenerate` option is set to `true`, then any existing remappings are
/// discarded and the remappings are generated from the dependencies in the config file.
///
/// Otherwise, existing remappings are kept, and depending on the action, a remapping entry is added
/// or removed. For the [`RemappingsAction::Update`] action, the existing remappings are merged with
/// the dependencies in the config file.
///
/// Dependencies are sorted alphabetically for consistency.
fn generate_remappings(
    action: &RemappingsAction,
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
    existing_remappings: &[(&str, &str)],
) -> Result<Vec<String>> {
    let mut new_remappings = Vec::new();
    if soldeer_config.remappings_regenerate {
        debug!("ignoring existing remappings and recreating from config");
        let (dependencies, _) = read_config_deps(&paths.config)?;
        new_remappings = remappings_from_deps(&dependencies, paths, soldeer_config)?
            .into_iter()
            .map(|i| i.remapping_string)
            .collect();
    } else {
        match &action {
            RemappingsAction::Remove(remove_dep) => {
                debug!(dep:% = remove_dep; "trying to remove dependency from remappings");
                // only keep items not matching the dependency to remove
                if let Ok(remove_og) = get_install_dir_relative(remove_dep, paths) {
                    for (existing_remapped, existing_og) in existing_remappings {
                        // TODO: make the detection smarter, and match on any path where the version
                        // is semver-compatible too.
                        if !existing_og.trim_end_matches('/').starts_with(&remove_og) {
                            new_remappings.push(format!("{existing_remapped}={existing_og}"));
                        } else {
                            debug!(dep:% = remove_dep; "found existing remapping corresponding to dependency to remove");
                        }
                    }
                } else {
                    debug!(dep:% = remove_dep; "could not find a directory matching the dependency to remove");
                    for (remapped, og) in existing_remappings {
                        new_remappings.push(format!("{remapped}={og}"));
                    }
                }
            }
            RemappingsAction::Add(add_dep) => {
                debug!(dep:% = add_dep; "adding remapping for dependency if necessary");
                // we only add the remapping if it's not already existing, otherwise we keep the old
                // remapping
                let add_dep_remapped = format_remap_name(soldeer_config, add_dep);
                let add_dep_og = get_install_dir_relative(add_dep, paths)?;
                let mut found = false; // whether a remapping existed for that dep already
                for (existing_remapped, existing_og) in existing_remappings {
                    new_remappings.push(format!("{existing_remapped}={existing_og}"));
                    if existing_og.trim_end_matches('/').starts_with(&add_dep_og) {
                        debug!(dep:% = add_dep; "remapping exists already, skipping");
                        found = true;
                    }
                }
                if !found {
                    debug!(dep:% = add_dep; "remapping not found, adding it");
                    new_remappings.push(format!("{add_dep_remapped}={add_dep_og}/"));
                }
            }
            RemappingsAction::Update => {
                // This is where we end up in the `update` command if we don't want to re-generate
                // all remappings. We need to merge existing remappings with the full list of deps.
                // We generate all remappings from the dependencies, then replace existing items.
                debug!("updating remappings, merging existing ones with the ones generated from config");
                let (dependencies, _) = read_config_deps(&paths.config)?;
                let new_remappings_info =
                    remappings_from_deps(&dependencies, paths, soldeer_config)?;
                if existing_remappings.is_empty() {
                    debug!("no existing remappings, using the ones from config");
                    new_remappings =
                        new_remappings_info.into_iter().map(|i| i.remapping_string).collect();
                } else {
                    let mut existing_remappings = Vec::from(existing_remappings);
                    for RemappingInfo { remapping_string: item, dependency: dep } in
                        new_remappings_info
                    {
                        debug!(dep:% = dep; "trying to find a matching existing remapping for config item");
                        let (_, item_og) =
                            item.split_once('=').expect("remappings should have two parts");
                        // try to find all existing items pointing to a matching dependency folder
                        let mut found = false;
                        existing_remappings.retain(|(existing_remapped, existing_og)| {
                            // only keep the first two components of the path (`dependencies`
                            // folder and the dependency folder)
                            let path: PathBuf =
                                PathBuf::from(existing_og).components().take(2).collect();
                            // if path matches, we should update the item's path with the new
                            // one and add it to the final list
                            if path_matches(&dep, &path) {
                                debug!(path = existing_og; "existing remapping matches the config item");
                                let path: PathBuf =
                                    PathBuf::from(existing_og).components().take(2).collect();
                                let existing_og_updated = existing_og.replace(
                                    path.to_slash_lossy().as_ref(),
                                    item_og.trim_end_matches('/'),
                                );
                                debug!(new_path = existing_og_updated; "updated remapping path");
                                new_remappings
                                    .push(format!("{existing_remapped}={existing_og_updated}"));
                                found = true;
                                // we remove this item from the existing remappings list as it's
                                // been processed
                                return false;
                            }
                            // keep this item to add it to the remappings again later
                            true
                        });
                        if !found {
                            debug!(dep:% = dep;"no existing remapping found for config item, adding it");
                            new_remappings.push(item);
                        }
                    }
                    // add extra existing remappings back
                    for (existing_remapped, existing_og) in existing_remappings {
                        debug!(path = existing_og; "adding extra remapping which was existing but didn't match a config item");
                        new_remappings.push(format!("{existing_remapped}={existing_og}"));
                    }
                }
            }
        }
    }

    // sort the remappings
    new_remappings.sort_unstable();
    Ok(new_remappings)
}

#[derive(Debug, Clone, From)]
struct RemappingInfo {
    remapping_string: String,
    dependency: Dependency,
}

/// Generate remappings from the dependencies list.
///
/// The remappings are generated in the form `alias/=path/`, where `alias` is the dependency name
/// with an optional prefix and version requirement suffix, and `path` is the relative path to the
/// dependency folder.
fn remappings_from_deps(
    dependencies: &[Dependency],
    paths: &Paths,
    soldeer_config: &SoldeerConfig,
) -> Result<Vec<RemappingInfo>> {
    dependencies
        .par_iter()
        .map(|dependency| {
            let dependency_name_formatted = format_remap_name(soldeer_config, dependency); // contains trailing slash
            let relative_path = get_install_dir_relative(dependency, paths)?;
            Ok((format!("{dependency_name_formatted}={relative_path}/"), dependency.clone()).into())
        })
        .collect::<Result<Vec<RemappingInfo>>>()
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

/// Format a TOML array as a multi-line array with indentation in case there is more than one
/// element.
///
/// # Examples
///
/// ```toml
/// [profile.default]
/// remappings = []
/// ```
///
/// ```toml
/// [profile.default]
/// remappings = ["lib1-1.0.0/=dependencies/lib1-1.0.0/"]
/// ```
///
/// ```toml
/// [profile.default]
/// remappings = [
///     "lib1-1.0.0/=dependencies/lib1-1.0.0/",
///     "lib2-2.0.0/=dependencies/lib2-2.0.0/",
/// ]
/// ```
fn format_array(array: &mut Array) {
    array.fmt();
    if (0..=1).contains(&array.len()) {
        array.set_trailing("");
        array.set_trailing_comma(false);
    } else {
        for item in array.iter_mut() {
            item.decor_mut().set_prefix("\n    ");
        }
        array.set_trailing("\n");
        array.set_trailing_comma(true);
    }
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

        let (dependencies, _) = read_config_deps(&paths.config).unwrap();
        let res = remappings_from_deps(&dependencies, &paths, &SoldeerConfig::default());
        assert!(res.is_ok(), "{res:?}");
        let res = res.unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].remapping_string, "dep1-^1.0.0/=dependencies/dep1-1.1.1/");
        assert_eq!(res[1].remapping_string, "dep2-2.0.0/=dependencies/dep2-2.0.0/");
        assert_eq!(res[2].remapping_string, "dep3-foobar/=dependencies/dep3-foobar/");
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

        // extra entries are kep
        let existing_deps = vec![
            ("lib1-1.0.0/", "dependencies/lib1-1.0.0/"),
            ("lib2-2.0.0/", "dependencies/lib2-2.0.0/"),
            ("lib3/", "dependencies/lib3/"),
        ];
        let res = generate_remappings(&RemappingsAction::Update, &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec![
                "lib1-1.0.0/=dependencies/lib1-1.0.0/",
                "lib2-2.0.0/=dependencies/lib2-2.0.0/",
                "lib3/=dependencies/lib3/"
            ]
        );
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

    #[test]
    fn test_edit_remappings_soldeer_config() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1.0.0"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib1-1.0.0")).unwrap();
        // the config gets ignored in this case
        let config =
            SoldeerConfig { remappings_location: RemappingsLocation::Config, ..Default::default() };
        let res = edit_remappings(&RemappingsAction::Update, &config, &paths);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&paths.remappings).unwrap();
        assert_eq!(contents, "lib1-1.0.0/=dependencies/lib1-1.0.0/\n");
    }

    #[test]
    fn test_generate_remappings_update_semver_custom() {
        let dir = testdir!();
        let contents = r#"[dependencies]
lib1 = "1"
lib2 = "2"
"#;
        fs::write(dir.join("soldeer.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        // libs have been updated to newer versions
        fs::create_dir_all(paths.dependencies.join("lib1-1.2.0")).unwrap();
        fs::create_dir_all(paths.dependencies.join("lib2-2.1.0")).unwrap();
        let config = SoldeerConfig::default();
        // all entries are customized, using an old version of the libs
        let existing_deps = vec![
            ("lib1-1/", "dependencies/lib1-1.1.1/src/"), // customize right part
            ("lib2/", "dependencies/lib2-2.0.1/src/"),   // customize both sides
        ];
        let res = generate_remappings(&RemappingsAction::Update, &paths, &config, &existing_deps);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            vec!["lib1-1/=dependencies/lib1-1.2.0/src/", "lib2/=dependencies/lib2-2.1.0/src/"]
        );
    }

    #[test]
    fn test_generate_remappings_duplicates() {
        let dir = testdir!();
        let contents = r#"[profile.default]
remappings = [
    "@openzeppelin-contracts/=dependencies/@openzeppelin-contracts-5.0.2/",
    "@openzeppelin/contracts/=dependencies/@openzeppelin-contracts-5.0.2/",
    "foo/=bar/",
]
libs = ["dependencies"]

[dependencies]
"@openzeppelin-contracts" = "5.0.2"
"#;
        fs::write(dir.join("foundry.toml"), contents).unwrap();
        let paths = Paths::from_root(&dir).unwrap();
        fs::create_dir_all(paths.dependencies.join("@openzeppelin-contracts-5.0.2")).unwrap();
        let res = remappings_foundry(
            &RemappingsAction::Update,
            &paths,
            &SoldeerConfig {
                remappings_location: RemappingsLocation::Config,
                ..Default::default()
            },
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(fs::read_to_string(dir.join("foundry.toml")).unwrap(), contents);
    }
}
