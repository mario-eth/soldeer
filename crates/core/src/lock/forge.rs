//! Vendored version of the `lockfile` module of `forge`.
//!
//! Slightly adapted to reduce dependencies.

use log::debug;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::errors::LockError;

use super::Result;

pub const FOUNDRY_LOCK: &str = "foundry.lock";

/// A type alias for a HashMap of dependencies keyed by relative path to the submodule dir.
pub type DepMap = HashMap<PathBuf, DepIdentifier>;

/// A lockfile handler that keeps track of the dependencies and their current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    /// A map of the dependencies keyed by relative path to the submodule dir.
    #[serde(flatten)]
    deps: DepMap,
    /// Absolute path to the lockfile.
    #[serde(skip)]
    lockfile_path: PathBuf,
}

impl Lockfile {
    /// Create a new [`Lockfile`] instance.
    ///
    /// `project_root` is the absolute path to the project root.
    ///
    /// You will need to call [`Lockfile::read`] or [`Lockfile::sync`] to load the lockfile.
    pub fn new(project_root: &Path) -> Self {
        Self { deps: HashMap::default(), lockfile_path: project_root.join(FOUNDRY_LOCK) }
    }

    /// Loads the lockfile from the project root.
    ///
    /// Throws an error if the lockfile does not exist.
    pub fn read(&mut self) -> Result<()> {
        if !self.lockfile_path.exists() {
            return Err(LockError::FoundryLockMissing);
        }

        let lockfile_str = fs::read_to_string(&self.lockfile_path)?;

        self.deps = serde_json::from_str(&lockfile_str)?;

        debug!(lockfile:? = self.deps; "loaded lockfile");

        Ok(())
    }

    /// Get the [`DepIdentifier`] for a submodule at a given path.
    pub fn get(&self, path: &Path) -> Option<&DepIdentifier> {
        self.deps.get(path)
    }

    /// Returns the num of dependencies in the lockfile.
    pub fn len(&self) -> usize {
        self.deps.len()
    }

    /// Returns whether the lockfile is empty.
    pub fn is_empty(&self) -> bool {
        self.deps.is_empty()
    }

    /// Returns an iterator over the lockfile.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &DepIdentifier)> {
        self.deps.iter()
    }

    pub fn exists(&self) -> bool {
        self.lockfile_path.exists()
    }
}

// Implement .iter() for &LockFile

/// Identifies whether a dependency (submodule) is referenced by a branch,
/// tag or rev (commit hash).
///
/// Each enum variant consists of an `r#override` flag which is used in `forge update` to decide
/// whether to update a dep or not. This flag is skipped during serialization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DepIdentifier {
    /// `name` of the branch and the `rev` it is currently pointing to.
    #[serde(rename = "branch")]
    Branch { name: String, rev: String },

    /// Release tag `name` and the `rev` it is currently pointing to.
    #[serde(rename = "tag")]
    Tag { name: String, rev: String },

    /// Commit hash `rev` the submodule is currently pointing to.
    #[serde(rename = "rev", untagged)]
    Rev { rev: String },
}

impl DepIdentifier {
    /// Get the commit hash of the dependency.
    pub fn rev(&self) -> &str {
        match self {
            Self::Branch { rev, .. } => rev,
            Self::Tag { rev, .. } => rev,
            Self::Rev { rev, .. } => rev,
        }
    }

    /// Get the name of the dependency.
    ///
    /// In case of a Rev, this will return the commit hash.
    pub fn name(&self) -> &str {
        match self {
            Self::Branch { name, .. } => name,
            Self::Tag { name, .. } => name,
            Self::Rev { rev, .. } => rev,
        }
    }

    /// Get the name/rev to checkout at.
    pub fn checkout_id(&self) -> &str {
        match self {
            Self::Branch { name, .. } => name,
            Self::Tag { name, .. } => name,
            Self::Rev { rev, .. } => rev,
        }
    }

    /// Returns whether the dependency is a branch.
    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch { .. })
    }
}

impl std::fmt::Display for DepIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Branch { name, rev, .. } => write!(f, "branch={name}@{rev}"),
            Self::Tag { name, rev, .. } => write!(f, "tag={name}@{rev}"),
            Self::Rev { rev, .. } => write!(f, "rev={rev}"),
        }
    }
}
