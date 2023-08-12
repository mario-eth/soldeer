use std::fmt::{ self };
use chrono::{ DateTime, Utc };
use serde_derive::{ Serialize, Deserialize };
use rusqlite::{ Connection, Result, Error };

#[derive(Deserialize, Serialize)]
pub struct Version {
    pub repository: String,
    pub version: String,
    pub last_updated: DateTime<Utc>,
}

pub fn get_versions_for_repo_from_db(repository: String) -> Result<Vec<String>, Error> {
    let conn = Connection::open("repositories.db")?;

    conn.execute(
        "create table if not exists versions (
             id integer primary key,
             repository text not null,
             version text not null,
             last_updated datetime not null
         )",
        ()
    )?;
    let mut stmt: rusqlite::Statement<'_> = conn.prepare(
        "SELECT version from versions where repository = ?1"
    )?;

    let versions = stmt.query_map([&repository], |row| { Ok(row.get(0)?) })?;

    return Ok(
        versions.map(|version: std::result::Result<String, Error>| version.unwrap()).collect()
    );
}

pub fn insert_version_into_db(version: Version) -> Result<(), Error> {
    println!("Inserting version {:?} into db for {:?}", version.version, version.repository);
    let conn = Connection::open("repositories.db")?;

    conn.execute(
        "create table if not exists versions (
             id integer primary key,
             repository text not null unique,
             version text not null unique,
             last_updated datetime not null
         )",
        ()
    )?;

    let mut stmt: rusqlite::Statement<'_> = conn.prepare(
        "INSERT INTO versions (repository, version, last_updated) VALUES (?1, ?2, ?3)"
    )?;

    stmt.execute(&[&version.repository, &version.version, &version.last_updated.to_string()])?;

    return Ok(());
}

#[derive(Debug, Clone)]
pub struct NotFound;

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "file not found")
    }
}
