//! Git operations using gitoxide library.
//!
//! This module contains functions that use the gitoxide (gix) library to perform
//! git operations without requiring an external git binary.

use crate::errors::GitError;
use gix::{
    bstr::{BStr, BString},
    error::Error as GixError,
    path::{into_bstr, to_unix_separators_on_windows},
    refs::{
        Target,
        transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog},
    },
    remote,
    worktree::{stack::state::attributes::Source, state},
};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
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

/// Check if there are any differences between the working tree and a specific revision.
///
/// This is equivalent to `git diff --exit-code <rev>`. Returns `true` if there are
/// differences, `false` if the working tree matches the revision.
///
/// As for git, untracked files are ignored.
pub async fn has_diff(repo_path: impl AsRef<Path>, rev: impl Into<String>) -> Result<bool> {
    let repo_path = repo_path.as_ref().to_path_buf();
    let rev: String = rev.into();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).gix_err()?;

        // Resolve the revision to a tree OID
        let tree_id = repo
            .rev_parse_single(rev.as_bytes())
            .gix_err()?
            .object()
            .gix_err()?
            .peel_to_tree()
            .gix_err()?
            .id;

        // Compare the rev's tree against the index and worktree
        let has_changes = repo
            .status(gix::progress::Discard)
            .gix_err()?
            .head_tree(tree_id)
            .index_worktree_options_mut(|opts| {
                opts.dirwalk_options = None; // skip untracked files
            })
            .into_iter(None::<BString>)
            .gix_err()?
            .next()
            .is_some();

        Ok(has_changes)
    })
    .await?
}

/// Check out a specific ref (branch, tag, or commit) in a repository.
///
/// This is equivalent to `git checkout <identifier>`. The worktree and index
/// are updated to match the target commit's tree, and HEAD is set to a detached
/// state pointing to the resolved commit.
pub async fn checkout(repo_path: impl AsRef<Path>, identifier: &str) -> Result<()> {
    let repo_path = repo_path.as_ref().to_path_buf();
    let identifier = identifier.to_string();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).gix_err()?;

        // Resolve identifier (branch/tag/commit) to a commit.
        // Fall back to `<remote>/<identifier>` for remote tracking branches, mirroring
        // git checkout's DWIM behavior in freshly cloned repos.
        let commit = repo
            .rev_parse_single(identifier.as_bytes())
            .or_else(|e| {
                let Ok(remote) = find_remote_name(&repo) else {
                    return Err(e);
                };
                repo.rev_parse_single(format!("{remote}/{identifier}").as_bytes())
            })
            .gix_err()?
            .object()
            .gix_err()?
            .peel_to_commit()
            .gix_err()?;
        let commit_id = commit.id;
        let tree_id = commit.tree_id().gix_err()?.detach();

        let workdir = repo.workdir().ok_or(GitError::BareRepository)?.to_path_buf();

        // Clear the worktree (everything except .git) to prepare for checkout
        for entry in std::fs::read_dir(&workdir)
            .map_err(|e| GitError::IOError { path: workdir.clone(), source: e })?
        {
            let Ok(entry) = entry else {
                continue;
            };
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| GitError::IOError { path: path.clone(), source: e })?;
            } else {
                std::fs::remove_file(&path)
                    .map_err(|e| GitError::IOError { path: path.clone(), source: e })?;
            }
        }

        // Build index from the target tree
        let mut index = repo.index_from_tree(&tree_id).gix_err()?;

        // Checkout the tree to the worktree
        let mut opts = repo.checkout_options(Source::IdMapping).gix_err()?;
        opts.destination_is_initially_empty = true;

        let should_interrupt = AtomicBool::new(false);
        state::checkout(
            &mut index,
            &workdir,
            repo.objects.clone().into_arc().gix_err()?,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &should_interrupt,
            opts,
        )
        .gix_err()?;

        // Write the updated index to disk
        index.write(Default::default()).gix_err()?;

        // Update HEAD to point to the checked-out commit (detached)
        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: format!("checkout: moving to {identifier}").into(),
                },
                expected: PreviousValue::Any,
                new: Target::Object(commit_id),
            },
            name: "HEAD".try_into().expect("HEAD is a valid ref name"),
            deref: false,
        })
        .gix_err()?;

        Ok(())
    })
    .await?
}

/// Get the default branch name for the default remote.
///
/// This is equivalent to `git symbolic-ref refs/remotes/[default_remote]/HEAD --short`.
pub async fn get_default_branch(repo_path: impl AsRef<Path>) -> Result<String> {
    let repo_path = repo_path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&repo_path).gix_err()?;
        let remote_name = find_remote_name(&repo)?;
        let ref_name = format!("{remote_name}/HEAD");
        let reference = repo.find_reference(&ref_name).gix_err()?;
        let target = reference.target();
        let target_name = target.try_name().ok_or_else(|| {
            GixError::from_error(std::io::Error::other(format!(
                "{ref_name} is not a symbolic reference"
            )))
        })?;
        let shortened = target_name.shorten().to_string();
        Ok(shortened.strip_prefix(&format!("{remote_name}/")).unwrap_or(&shortened).to_string())
    })
    .await?
}

/// Find the name of the default fetch remote for a repository.
///
/// Uses gix's `remote_default_name` which prefers the only remote if there's just one,
/// or falls back to `origin` when it's defined and multiple remotes exist.
pub fn find_remote_name(repo: &gix::Repository) -> Result<Cow<'_, BStr>> {
    repo.remote_default_name(remote::Direction::Fetch).ok_or(GitError::NoRemote)
}

/// Create a BStr from a path, which is what gix expects.
pub fn make_path_bstr(path: &Path) -> Cow<'_, BStr> {
    let bstr = into_bstr(path);
    to_unix_separators_on_windows(bstr)
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
