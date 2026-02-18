//! Git operations using gitoxide library.
//!
//! This module contains functions that use the gitoxide (gix) library to perform
//! git operations without requiring an external git binary.

use crate::errors::GitError;
use gix::{bstr::BStr, error::Error as GixError};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, GitError>;

/// Get the current HEAD commit hash.
///
/// This is equivalent to `git rev-parse --verify HEAD`.
pub async fn get_head_commit(repo_path: impl AsRef<Path>) -> Result<String> {
    let repo_path = repo_path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).gix_err()?;
        let head_id = repo.head().gix_err()?.into_peeled_id().gix_err()?;
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
        let repo = gix::open(&repo_path).gix_err()?;
        let mut index = repo.open_index().gix_err()?;

        // Convert the path to be relative to the repository root
        let relative_path = if path_to_remove.is_absolute() {
            path_to_remove
                .strip_prefix(&repo_path)
                .map_err(|_| GitError::InvalidPath(path_to_remove.clone()))?
        } else {
            path_to_remove.as_path()
        };

        let entry_idx = index
            .entry_index_by_path(&make_path_bstr(relative_path))
            .map_err(|_| GitError::PathNotInIndex(path_to_remove))?;

        index.remove_entry_at_index(entry_idx);
        index.write(Default::default()).gix_err()?;
        Ok(())
    })
    .await?
}

/// Get the top-level directory (worktree root) of a git repository.
///
/// This is equivalent to `git rev-parse --show-toplevel`. It discovers the repository
/// at or above the given path and returns the canonicalized worktree root path.
///
/// Returns `None` if the path is not inside a git repository.
pub async fn get_toplevel(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = gix::discover(&path).ok()?;
        let work_dir = repo.workdir()?;
        dunce::canonicalize(work_dir).ok()
    })
    .await
    .ok()
    .flatten()
}

/// Create a BStr from a path, which is what gix expects.
pub fn make_path_bstr(path: &Path) -> Cow<'_, BStr> {
    let bstr = gix::path::into_bstr(path);
    gix::path::to_unix_separators_on_windows(bstr)
}

/// Extension trait to ergonomically convert an error into a [`gix::error::Error`](GixError).
trait GixErr<T> {
    fn gix_err(self) -> std::result::Result<T, GixError>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> GixErr<T> for std::result::Result<T, E> {
    #[track_caller]
    fn gix_err(self) -> std::result::Result<T, GixError> {
        self.map_err(GixError::from_error)
    }
}
