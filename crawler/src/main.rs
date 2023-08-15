mod npm;
mod utils;
mod db;
mod manager;

use npm::{ load_repositories, npm_retrieve_versions, retrieve_version };
use npm::LoadError;
use db::{ get_versions_for_repo_from_db, insert_version_into_db, Version };
use rusqlite::Error;
use chrono::Utc;
use manager::{ zip_version, push_to_repository };
fn main() {
    let repositories: Vec<String> = load_repositories()
        .map_err(|err: LoadError| {
            println!("{:?}", err);
        })
        .unwrap();

    for repository in repositories {
        let existing_versions: Vec<String> = get_versions_for_repo_from_db(repository.clone())
            .map_err(|err: Error| {
                println!("{:?}", err);
            })
            .unwrap();
        let versions: Vec<String> = npm_retrieve_versions(&repository)
            .map_err(|err: LoadError| {
                println!("{:?}", err);
            })
            .unwrap();

        for version in versions {
            if existing_versions.contains(&version) {
                continue;
            }
            retrieve_version(&repository, &version)
                .map_err(|err| {
                    println!("{:?}", err);
                })
                .unwrap();
            let version_to_insert = Version {
                repository: repository.clone(),
                version: version.clone(),
                last_updated: Utc::now(),
            };

            zip_version(&repository, &version);
            push_to_repository(&repository, &version);

            insert_version_into_db(version_to_insert)
                .map_err(|err: Error| {
                    println!("{:?}", err);
                })
                .unwrap();
        }
    }
}
