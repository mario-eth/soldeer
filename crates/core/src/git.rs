//! Git operations using gitoxide library.
//!
//! This module contains functions that use the gitoxide (gix) library to perform
//! git operations without requiring an external git binary.

use crate::errors::GitError;
use gix::error::Error as GixError;
use std::path::Path;

pub type Result<T> = std::result::Result<T, GitError>;

/// Get the current HEAD commit hash.
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
