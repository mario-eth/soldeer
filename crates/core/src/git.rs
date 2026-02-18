//! Git operations using gitoxide library.
//!
//! This module contains functions that use the gitoxide (gix) library to perform
//! git operations without requiring an external git binary.

use crate::errors::GitError;
use gix::{
    bstr::{BStr, ByteSlice as _},
    error::Error as GixError,
    hash, index,
    path::to_unix_separators_on_windows,
};
use std::{borrow::Cow, os::unix::ffi::OsStrExt as _, path::Path};

pub type Result<T> = std::result::Result<T, GitError>;

/// Get the current HEAD commit hash.
///
/// This is equivalent to `git rev-parse --verify HEAD`.
pub async fn get_head_commit(repo_path: impl AsRef<Path>) -> Result<String> {
    let repo_path = repo_path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).map_err(GixError::from_error)?;

        let head_id = repo
            .head()
            .map_err(GixError::from_error)?
            .try_peel_to_id()
            .map_err(GixError::from_error)?
            .ok_or_else(|| GitError::UnbornHead(repo_path))?;

        Ok(head_id.to_string())
    })
    .await?
}

/// Remove a path from the git index.
///
/// This is equivalent to `git rm --cached <path>` (without removing the file from disk).
/// The caller is responsible for removing the file from the filesystem if needed.
pub async fn remove_from_index(
    repo_path: impl AsRef<Path>,
    path_to_remove: impl AsRef<Path>,
) -> Result<()> {
    let repo_path = repo_path.as_ref().to_path_buf();
    let path_to_remove = path_to_remove.as_ref().to_path_buf();

    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).map_err(GixError::from_error)?;

        // Get the index file path and load it
        let index_path = repo.index_path();
        let mut index = index::File::at(&index_path, hash::Kind::Sha1, false, Default::default())
            .map_err(GixError::from_error)?;

        // Convert the path to be relative to the repository root
        let relative_path = if path_to_remove.is_absolute() {
            path_to_remove
                .strip_prefix(&repo_path)
                .map_err(|_| GitError::InvalidPath(path_to_remove.clone()))?
                .to_path_buf()
        } else {
            path_to_remove.clone()
        };

        let entry_idx = index
            .entry_index_by_path(&make_path_bstr(&relative_path))
            .map_err(|_| GitError::PathNotInIndex(path_to_remove))?;

        index.remove_entry_at_index(entry_idx);
        index.write(Default::default()).map_err(GixError::from_error)?;
        Ok(())
    })
    .await?
}

/// Create a BStr from a path, which is what gix expects.
pub fn make_path_bstr<'a>(path: &'a Path) -> Cow<'a, BStr> {
    let path = path.as_os_str().as_bytes();
    to_unix_separators_on_windows(path.as_bstr())
}
