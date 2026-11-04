use anyhow::{bail, Context, Result};
use indicatif::MultiProgress;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::add::pr::{self, ResolvedBranch};
use super::common::ambiguous_name_error;
use super::context::CommandContext;
use crate::color::ColorMode;
use crate::domain::worktree::{NameResolution, WorktreeEntry, WorktreeList};
use crate::hooks::{self, FilePolicy, HookContext};
use crate::integrations::fzf::{self, FzfPicker};
use crate::integrations::gh::{self, GhClient, PrInfo};
use crate::integrations::git::{worktree_state, GitClient, RealGitClient};
use crate::integrations::zoxide::{self, ZoxideClient};

enum Source {
    Plain,
    Issue,
    Pull { number: u32, info: PrInfo },
}
struct Target {
    branch: String,
    source: Source,
}
struct Snapshot {
    entry: WorktreeEntry,
    admin: PathBuf,
}

/// Navigate to an existing checkout or switch a selected clean linked worktree.
///
/// # Errors
/// Returns an error for invalid targets, unsafe members, failed checkout or setup.
pub fn cmd_switch(
    branch: Option<&str>,
    member: Option<&str>,
    verbose: bool,
    color: ColorMode,
) -> Result<()> {
    let input = match branch {
        Some(branch) => branch.to_string(),
        None => crate::stdin::try_read_stdin_first()?
            .context("branch name required (provide as argument or via stdin)")?,
    };
    let ctx = CommandContext::new_strict(color)?;
    let target = discover(&ctx, &input)?;
    let list = load(&ctx, false)?;
    let existing: Vec<_> = list
        .entries()
        .iter()
        .filter(|entry| entry.navigation_branch() == Some(&target.branch))
        .collect();
    match existing.as_slice() {
        [entry] => {
            verify_identity(
                &ctx,
                entry,
                list.main().is_some_and(|main| main.path == entry.path),
            )?;
            return print_path(&entry.path);
        }
        [] => {}
        _ => bail!(
            "Branch '{}' is present in multiple worktrees: {}",
            target.branch,
            existing
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
    let caller_head = if matches!(target.source, Source::Issue) {
        Some(
            ctx.git
                .rev_parse(&["rev-parse", "--verify", "HEAD^{commit}"], None)?
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    let list = load(&ctx, true)?;
    let Some(snapshot) = select_member(&ctx, &list, member)? else {
        return Ok(());
    };
    revalidate(&ctx, &snapshot)?;
    let retained = if matches!(target.source, Source::Pull { .. }) {
        "; any fetched or fast-forwarded PR refs were retained"
    } else {
        ""
    };
    let resolved = materialize(&ctx, &target, caller_head.as_deref())
        .with_context(|| format!("Branch preparation failed{retained}"))?;
    revalidate(&ctx, &snapshot).with_context(|| {
        format!("Could not revalidate the worktree after branch preparation{retained}")
    })?;
    let path = Path::new(&snapshot.entry.path);
    checkout(
        &ctx,
        path,
        &resolved,
        !matches!(target.source, Source::Plain),
    )
    .with_context(|| {
        format!(
            "Checkout did not complete at {}{retained}. Inspect the worktree before retrying",
            path.display()
        )
    })?;
    finish_setup(&ctx, path, &resolved, verbose)?;
    print_path(&snapshot.entry.path)
}

fn finish_setup(
    ctx: &CommandContext,
    path: &Path,
    resolved: &ResolvedBranch,
    verbose: bool,
) -> Result<()> {
    resolved.record_provenance(&ctx.git, &ctx.repo_root, path)?;
    let actions = &ctx.config.hooks.create;
    if !actions.run.is_empty() || !actions.copy.is_empty() || !actions.link.is_empty() {
        let mut context = HookContext::new(
            path,
            &ctx.repo_root,
            ctx.worktree_root().as_deref(),
            verbose,
        )
        .with_context(|| {
            format!(
                "Branch switched at {}, but hook preparation failed",
                path.display()
            )
        })?;
        context.file_policy = FilePolicy::Sync { overwrite: false };
        hooks::execute_hooks_with_mp(actions, &context, ctx.color_mode, "  ", &MultiProgress::new())
            .with_context(|| format!("Switched worktree preserved at {} after hook failure; use ofsht sync with this worktree to retry setup", path.display()))?;
    }
    if ctx.config.integrations.zoxide.enabled && zoxide::is_zoxide_available() {
        zoxide::RealZoxideClient.add(path).with_context(|| {
            format!(
                "Branch switched at {}, but zoxide registration failed",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn discover(ctx: &CommandContext, input: &str) -> Result<Target> {
    let target = match gh::BranchInput::parse(input) {
        gh::BranchInput::Github(number) if ctx.config.integrations.gh.enabled => {
            let client = gh::RealGhClient;
            if !client.is_available() {
                bail!("GitHub CLI (gh) is not installed or available");
            }
            if let Ok(info) = client.pr_info(number) {
                Target {
                    branch: pr::branch_name(&ctx.git, &info, number, &ctx.repo_root)?,
                    source: Source::Pull { number, info },
                }
            } else {
                client
                    .issue_info(number)
                    .with_context(|| format!("#{number} is not a valid issue or pull request"))?;
                Target {
                    branch: gh::build_issue_branch(number),
                    source: Source::Issue,
                }
            }
        }
        gh::BranchInput::Github(_) => {
            eprintln!(
                "GitHub integration is disabled. Treating '{input}' as a literal branch name."
            );
            Target {
                branch: input.to_string(),
                source: Source::Plain,
            }
        }
        gh::BranchInput::Plain(branch) => Target {
            branch,
            source: Source::Plain,
        },
    };
    if target.branch.starts_with('-') || matches!(target.branch.as_str(), "@" | "HEAD") {
        bail!("Invalid branch name: {}", target.branch);
    }
    ctx.git
        .run(
            &["check-ref-format", &format!("refs/heads/{}", target.branch)],
            Some(&ctx.repo_root),
        )
        .context("Invalid branch name")?;
    Ok(target)
}

fn load(ctx: &CommandContext, full_status: bool) -> Result<WorktreeList> {
    let mut list = WorktreeList::parse(&RealGitClient::worktree_listing_z(&ctx.repo_root)?, None);
    worktree_state::inspect(&mut list, &ctx.repo_root, full_status);
    Ok(list)
}

fn select_member(
    ctx: &CommandContext,
    list: &WorktreeList,
    member: Option<&str>,
) -> Result<Option<Snapshot>> {
    let root = ctx.worktree_root();
    if let Some(name) = member {
        let current;
        let name = if name == "." {
            current = RealGitClient::worktree_toplevel(&std::env::current_dir()?)?;
            current.to_str().context("Worktree path is not UTF-8")?
        } else {
            name
        };
        let main = list
            .main()
            .context("git worktree list returned no entries")?;
        if name == "@" || main.navigation_branch() == Some(name) {
            bail!("Cannot reuse the main worktree; choose a linked worktree with --in");
        }
        return match list.resolve_name(name, root.as_deref()) {
            NameResolution::Found(entry) => reusable(ctx, entry).map(Some),
            NameResolution::Ambiguous(entries) => Err(ambiguous_name_error(name, &entries)),
            NameResolution::NotFound => bail!("No linked worktree found for --in {name}"),
        };
    }
    if !std::io::stdin().is_terminal() {
        bail!("Worktree selection requires a terminal; specify --in <worktree>");
    }
    if !ctx.config.integrations.fzf.enabled || !fzf::is_fzf_available() {
        bail!("Provide --in <worktree> or enable and install fzf");
    }
    let mut candidates: Vec<_> = list
        .non_main()
        .iter()
        .filter_map(|entry| reusable(ctx, entry).ok())
        .collect();
    if candidates.is_empty() {
        bail!("No reusable linked worktrees; inspect them with ofsht ls --status");
    }
    let items: Vec<_> = fzf::format_worktree_items(list, root.as_deref())
        .into_iter()
        .filter(|item| {
            candidates
                .iter()
                .any(|candidate| candidate.entry.path == item.value)
        })
        .collect();
    let selected =
        fzf::RealFzfPicker::new(ctx.config.integrations.fzf.options.clone()).pick(&items, false)?;
    let Some(path) = selected.first() else {
        return Ok(None);
    };
    let index = candidates
        .iter()
        .position(|candidate| &candidate.entry.path == path)
        .context("Picker returned an unknown worktree")?;
    Ok(Some(candidates.remove(index)))
}

fn reusable(ctx: &CommandContext, entry: &WorktreeEntry) -> Result<Snapshot> {
    let state = &entry.state;
    if state.missing || state.prunable.is_some() {
        bail!("Worktree directory is missing or prunable: {}", entry.path);
    }
    if state.locked.is_some() {
        bail!("Worktree is locked: {}", entry.path);
    }
    if state.operation.is_some() {
        bail!("Worktree has an operation in progress: {}", entry.path);
    }
    if let Some(error) = &state.error {
        bail!("Cannot inspect worktree {}: {error}", entry.path);
    }
    if state.dirty != Some(false) {
        bail!("Worktree is not clean: {}", entry.path);
    }
    let admin = verify_identity(ctx, entry, false)?;
    if admin.join("sequencer").try_exists()? {
        bail!("Worktree has an operation in progress: {}", entry.path);
    }
    if entry.branch.is_none() {
        let head = entry
            .hash
            .as_deref()
            .context("Detached worktree has no commit")?;
        let refs = ctx.git.run(
            &[
                "for-each-ref",
                "--contains",
                head,
                "--count=1",
                "refs/heads",
                "refs/tags",
                "refs/remotes",
            ],
            Some(&ctx.repo_root),
        )?;
        if refs.trim().is_empty() {
            bail!("Detached worktree has commits without a shared branch or tag; create a reference before reusing {}", entry.path);
        }
    }
    Ok(Snapshot {
        entry: entry.clone(),
        admin,
    })
}

fn verify_identity(ctx: &CommandContext, entry: &WorktreeEntry, main: bool) -> Result<PathBuf> {
    let path = Path::new(&entry.path);
    let common = path
        .join(RealGitClient::common_git_dir(path)?)
        .canonicalize()?;
    let expected = ctx
        .repo_root
        .join(RealGitClient::common_git_dir(&ctx.repo_root)?)
        .canonicalize()?;
    if common != expected
        || RealGitClient::worktree_toplevel(path)?.canonicalize()? != path.canonicalize()?
    {
        bail!("Worktree repository identity changed: {}", entry.path);
    }
    let admin = if main {
        let admin = RealGitClient::worktree_git_dir(path)?.canonicalize()?;
        if admin != common {
            bail!("Main worktree identity changed");
        }
        admin
    } else {
        RealGitClient::linked_worktree_git_dir(path)?
    };
    let branch = optional_git(path, &["symbolic-ref", "--quiet", "HEAD"])?.map(|branch| {
        branch
            .strip_prefix("refs/heads/")
            .unwrap_or(&branch)
            .to_string()
    });
    let head = optional_git(path, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if branch != entry.branch || head != entry.hash {
        bail!("Worktree branch or HEAD changed: {}", entry.path);
    }
    Ok(admin)
}

fn optional_git(path: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .context("Failed to inspect Git worktree identity")?;
    match output.status.code() {
        Some(0) => {
            let text = String::from_utf8(output.stdout).context("Git identity is not UTF-8")?;
            Ok(Some(text.strip_suffix('\n').unwrap_or(&text).to_string()))
        }
        Some(1) => Ok(None),
        _ => bail!(
            "Failed to inspect Git worktree identity: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn revalidate(ctx: &CommandContext, snapshot: &Snapshot) -> Result<()> {
    let list = load(ctx, true)?;
    let entry = list
        .non_main()
        .iter()
        .find(|entry| entry.path == snapshot.entry.path)
        .context("Selected worktree registration disappeared")?;
    if entry.branch != snapshot.entry.branch || entry.hash != snapshot.entry.hash {
        bail!("Selected worktree branch or HEAD changed; retry selection");
    }
    let now = reusable(ctx, entry)?;
    if now.admin != snapshot.admin {
        bail!("Selected worktree administration identity changed");
    }
    Ok(())
}

fn materialize(
    ctx: &CommandContext,
    target: &Target,
    caller_head: Option<&str>,
) -> Result<ResolvedBranch> {
    match &target.source {
        Source::Pull { number, info } => {
            if pr::branch_name(&ctx.git, info, *number, &ctx.repo_root)? != target.branch {
                bail!("Pull request branch identity changed; retry");
            }
            let resolved = pr::process_pr(&ctx.git, info, *number, &ctx.repo_root, ctx.color_mode)?;
            if resolved.branch != target.branch {
                bail!("Pull request branch identity changed during preparation");
            }
            Ok(resolved)
        }
        Source::Issue => Ok(ResolvedBranch {
            branch: target.branch.clone(),
            start_point: if ctx
                .git
                .branch_exists(&target.branch, Some(&ctx.repo_root))?
            {
                None
            } else {
                Some(
                    caller_head
                        .context("Missing original caller commit")?
                        .to_string(),
                )
            },
            fork_pr: None,
        }),
        Source::Plain => Ok(ResolvedBranch {
            branch: target.branch.clone(),
            start_point: None,
            fork_pr: None,
        }),
    }
}

fn checkout(
    ctx: &CommandContext,
    path: &Path,
    target: &ResolvedBranch,
    create_missing: bool,
) -> Result<()> {
    let mut command = Command::new("git");
    command
        .current_dir(path)
        .args(["switch", "--no-overwrite-ignore", "--no-recurse-submodules"]);
    if create_missing
        && !ctx
            .git
            .branch_exists(&target.branch, Some(&ctx.repo_root))?
    {
        command.args([
            "-c",
            &target.branch,
            "--",
            target
                .start_point
                .as_deref()
                .context("Target branch disappeared; retry")?,
        ]);
    } else {
        command.arg("--").arg(&target.branch);
    }
    let output = command.output().context("Failed to execute git switch")?;
    if !output.status.success() {
        bail!(
            "git switch failed: {}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    eprintln!("Switched {} to {}", path.display(), target.branch);
    Ok(())
}

fn print_path(path: &str) -> Result<()> {
    crate::output::write_stdout(format_args!("{path}\n"))
}
