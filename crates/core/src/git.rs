//! Git operations using gitoxide library.
//!
//! This module contains functions that use the gitoxide (gix) library to perform
//! git operations without requiring an external git binary.

use crate::errors::GitError;
use gix::{
    ObjectId,
    bstr::{BStr, BString},
    error::Error as GixError,
    path::{into_bstr, to_unix_separators_on_windows},
    refs::{
        FullName, Target,
        transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog},
    },
    remote,
    worktree::{stack::state::attributes::Source, state},
};
use std::{
    borrow::Cow,
    fs::File,
    io::Write as _,
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

/// Check out a specific ref (branch, tag, or commit) in a repository.
///
/// This is equivalent to `git checkout <identifier>`. The worktree and index are updated to match
/// the target commit's tree. HEAD handling mirrors git's behavior:
/// - Local branch name: HEAD becomes a symbolic ref to the branch
/// - Remote tracking branch (no local): creates a local tracking branch, HEAD points to it
/// - Tag, SHA, or other rev: HEAD is detached at the resolved commit
pub async fn checkout(repo_path: impl AsRef<Path>, identifier: impl Into<String>) -> Result<()> {
    let repo_path = repo_path.as_ref().to_path_buf();
    let identifier: String = identifier.into();
    tokio::task::spawn_blocking(move || {
        let mut repo = gix::open(&repo_path).gix_err()?;
        let workdir = repo.workdir().ok_or(GitError::BareRepository)?.to_path_buf();

        let target = resolve_checkout_target(&mut repo, &identifier)?;

        // Clear the worktree (everything except .git) and checkout the target tree
        clear_worktree(&workdir)?;
        let mut index = repo.index_from_tree(&target.tree_id).gix_err()?;
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
        index.write(Default::default()).gix_err()?;

        // Update HEAD
        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: format!("checkout: moving to {identifier}").into(),
                },
                expected: PreviousValue::Any,
                new: target.target,
            },
            name: "HEAD".try_into().expect("HEAD is a valid ref name"),
            deref: false,
        })
        .gix_err()?;

        Ok(())
    })
    .await?
}

#[derive(Debug)]
struct CheckoutTarget {
    tree_id: ObjectId,
    target: Target,
}

/// Resolve the checkout target following git's rules.
///
/// Resolution order:
/// 1. Local branch (`refs/heads/<identifier>`)
/// 2. Remote tracking branch: creates local tracking branch + config
/// 3. General rev via `rev_parse_single` (tag, SHA, etc.): detached
fn resolve_checkout_target(repo: &mut gix::Repository, identifier: &str) -> Result<CheckoutTarget> {
    // 1. Local branch
    if let Some(mut reference) =
        repo.try_find_reference(&format!("heads/{identifier}")).gix_err()?
    {
        let tree_id = reference.peel_to_tree().gix_err()?.id;
        return Ok(CheckoutTarget {
            tree_id,
            target: Target::Symbolic(
                format!("refs/heads/{identifier}").try_into().expect("valid ref name"),
            ),
        });
    }

    // 2. Remote tracking branch: create local branch with tracking config
    // Skip for "HEAD": refs/remotes/<remote>/HEAD is a symbolic ref, not a tracking branch
    if identifier != "HEAD" &&
        let Ok(remote_name) = find_remote_name(repo) &&
        let Some(mut reference) = repo
            .try_find_reference(format!("refs/remotes/{remote_name}/{identifier}").as_str())
            .gix_err()?
    {
        let (commit_id, tree_id) = {
            let commit = reference.peel_to_commit().gix_err()?;
            (commit.id, commit.tree_id().gix_err()?.detach())
        };

        // Create local branch ref pointing to the same commit
        let full_name: FullName =
            format!("refs/heads/{identifier}").try_into().expect("valid ref name");
        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: "branch: created from remote tracking branch".into(),
                },
                expected: PreviousValue::Any,
                new: Target::Object(commit_id),
            },
            name: full_name.clone(),
            deref: false,
        })
        .gix_err()?;

        // Set up tracking config
        write_branch_config(repo, identifier, &remote_name.to_string())?;

        return Ok(CheckoutTarget { tree_id, target: Target::Symbolic(full_name) });
    }

    // 3. General rev (tag, SHA, etc.)
    let commit = repo
        .rev_parse_single(identifier)
        .gix_err()?
        .object()
        .gix_err()?
        .peel_to_commit()
        .gix_err()?;
    let commit_id = commit.id;
    let tree_id = commit.tree_id().gix_err()?.detach();
    Ok(CheckoutTarget { tree_id, target: Target::Object(commit_id) })
}

/// Create a BStr from a path, which is what gix expects.
fn make_path_bstr(path: &Path) -> Cow<'_, BStr> {
    let bstr = into_bstr(path);
    to_unix_separators_on_windows(bstr)
}

/// Write branch tracking config (`branch.<name>.remote` and `branch.<name>.merge`) to the local
/// `.git/config` file.
fn write_branch_config(
    repo: &mut gix::Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<()> {
    let mut config = repo.config_snapshot_mut();
    let mut section = config
        .new_section("branch", Some(Cow::Owned(branch_name.into())))
        .expect("valid section name");
    section.push("remote".try_into().expect("valid key"), Some(remote_name.into()));
    let merge_value = format!("refs/heads/{branch_name}");
    section.push("merge".try_into().expect("valid key"), Some(merge_value.as_str().into()));
    // Persist to the local config file on disk
    let config_path = config.meta().path.as_deref().expect("local config has a path");
    let mut file = File::options()
        .create(false)
        .write(true)
        .open(config_path)
        .map_err(|e| GitError::IOError { path: config_path.to_path_buf(), source: e })?;
    file.write_all(config.detect_newline_style())
        .map_err(|e| GitError::IOError { path: config_path.to_path_buf(), source: e })?;
    config
        .write_to_filter(&mut file, |s| s.meta().source == gix::config::Source::Local)
        .map_err(|e| GitError::IOError { path: config_path.to_path_buf(), source: e })?;
    config.commit().gix_err()?;
    Ok(())
}

/// Remove all files and directories from the worktree, except `.git`.
fn clear_worktree(workdir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(workdir)
        .map_err(|e| GitError::IOError { path: workdir.to_path_buf(), source: e })?
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
    Ok(())
}

/// Find the name of the default fetch remote for a repository.
///
/// Uses gix's `remote_default_name` which prefers the only remote if there's just one,
/// or falls back to `origin` when it's defined and multiple remotes exist.
fn find_remote_name(repo: &gix::Repository) -> Result<Cow<'_, BStr>> {
    repo.remote_default_name(remote::Direction::Fetch).ok_or(GitError::NoRemote)
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
