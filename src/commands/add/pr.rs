use anyhow::{Context, Result};
use std::path::Path;

use crate::color::{self, ColorMode};
use crate::domain::worktree::WorktreeList;
use crate::integrations::gh::PrInfo;
use crate::integrations::git::GitClient;

#[derive(Debug)]
pub(super) struct ResolvedBranch {
    pub branch: String,
    pub start_point: Option<String>,
    pub fork_pr: Option<u32>,
}

impl ResolvedBranch {
    pub fn record_provenance(
        &self,
        git: &impl GitClient,
        repo: &Path,
        worktree: &Path,
    ) -> Result<()> {
        if let Some(number) = self.fork_pr {
            let key = format!("branch.{}.ofsht-pr", self.branch);
            git.run(
                &["config", "--local", &key, &number.to_string()],
                Some(repo),
            )
            .with_context(|| {
                format!(
                    "Worktree was created at {}, but its PR association could not be saved. \
                     Before repeating add, set local git config key {key:?} to {number}",
                    worktree.display()
                )
            })?;
        }
        Ok(())
    }
}

pub(super) fn process_pr(
    git: &impl GitClient,
    pr: &PrInfo,
    number: u32,
    repo: &Path,
    color_mode: ColorMode,
) -> Result<ResolvedBranch> {
    let head_ref = format!("refs/heads/{}", pr.head_ref_name);
    if pr.head_ref_name.starts_with('-') {
        anyhow::bail!("Invalid pull request branch: {}", pr.head_ref_name);
    }
    git.run(&["check-ref-format", &head_ref], Some(repo))
        .context("Invalid pull request branch")?;
    let fetch_ref = if pr.is_cross_repository {
        format!("refs/pull/{number}/head")
    } else {
        head_ref
    };
    git.fetch(&["fetch", "origin", &fetch_ref], Some(repo))?;
    // FETCH_HEAD is worktree-local; creation may run in a different worktree.
    let fetched = git
        .rev_parse(&["rev-parse", "--verify", "FETCH_HEAD"], Some(repo))?
        .trim()
        .to_string();
    eprintln!(
        "{}",
        color::success(
            color_mode,
            format!(
                "Fetched PR #{}: {}{}",
                pr.number,
                pr.title,
                if pr.is_cross_repository {
                    " (fork)"
                } else {
                    ""
                }
            )
        )
    );

    let branch = if pr.is_cross_repository {
        fork_branch(git, pr, number, repo)?
    } else {
        pr.head_ref_name.clone()
    };
    let start_point = if git.branch_exists(&branch, Some(repo))? {
        update_unused_branch(git, &branch, &fetched, repo, color_mode)?;
        None
    } else {
        let remote = format!("origin/{branch}");
        let remote_ref = format!("refs/remotes/{remote}");
        let tracks_fetched = !pr.is_cross_repository
            && git
                .rev_parse(&["rev-parse", "--verify", &remote_ref], Some(repo))
                .is_ok_and(|sha| sha.trim() == fetched);
        Some(if tracks_fetched { remote } else { fetched })
    };
    Ok(ResolvedBranch {
        branch,
        start_point,
        fork_pr: pr.is_cross_repository.then_some(number),
    })
}

fn fork_branch(git: &impl GitClient, pr: &PrInfo, number: u32, repo: &Path) -> Result<String> {
    let candidates = [
        pr.head_ref_name.clone(),
        format!("pr-{number}-{}", pr.head_ref_name.replace('/', "-")),
    ];
    let mut first_free = None;
    for candidate in &candidates {
        if git.branch_exists(candidate, Some(repo))? {
            let key = format!("branch.{candidate}.ofsht-pr");
            let owner = git.run(
                &["config", "--local", "--default", "", "--get", &key],
                Some(repo),
            )?;
            if owner.trim() == number.to_string() {
                return Ok(candidate.clone());
            }
        } else if first_free.is_none() {
            first_free = Some(candidate.clone());
        }
    }
    first_free.with_context(|| format!(
        "Local branches '{}' and '{}' already exist. Neither branch is associated with PR #{number}. \
         Rename an unrelated branch before trying again",
        candidates[0], candidates[1]
    ))
}

fn update_unused_branch(
    git: &impl GitClient,
    branch: &str,
    fetched: &str,
    repo: &Path,
    color_mode: ColorMode,
) -> Result<()> {
    let local_ref = format!("refs/heads/{branch}");
    let local = git.rev_parse(&["rev-parse", "--verify", &local_ref], Some(repo))?;
    if local.trim() == fetched {
        return Ok(());
    }
    let range = format!("{fetched}..{}", local.trim());
    let local_only = git.run(&["rev-list", "--count", &range], Some(repo))?;
    if local_only.trim() != "0" {
        eprintln!(
            "{}",
            color::warn(
                color_mode,
                format!(
                    "Local branch '{branch}' differs from the pull request; keeping local commits"
                )
            )
        );
        return Ok(());
    }
    let porcelain = git.list_worktrees(Some(repo))?;
    let list = WorktreeList::parse(&porcelain, None);
    if list
        .entries()
        .iter()
        .any(|entry| entry.branch.as_deref() == Some(branch))
    {
        eprintln!("{}", color::warn(color_mode, format!(
            "Local branch '{branch}' is behind the pull request; its checked-out worktree was not changed"
        )));
        return Ok(());
    }
    // A forced ref update could discard a concurrent commit or move an active worktree's branch.
    git.fetch(
        &["fetch", ".", &format!("{fetched}:{local_ref}")],
        Some(repo),
    )
    .with_context(|| format!("Could not fast-forward '{branch}'; its worktree was not created"))?;
    eprintln!(
        "{}",
        color::success(color_mode, format!("Fast-forwarded {branch}"))
    );
    Ok(())
}
