---
name: ofsht
description: Helps coding agents use the ofsht Git worktree management CLI safely, including add/create/list/cd/rm/sync/open workflows, hooks, shell integration awareness, and verification.
license: MIT
---

# ofsht

Use `ofsht` to manage Git worktrees when a repository already relies on it or when the user asks for `ofsht`, worktree automation, hook-based worktree setup, zoxide registration, fzf selection, or tmux worktree opening.

## When to use

- Prefer `ofsht` over raw `git worktree` when `.ofsht.toml` exists, the user mentions hooks/copy/link automation, or the task benefits from `ofsht` integrations.
- Use raw `git` only for read-only inspection or when `ofsht` is unavailable and the user has not required it.
- Before changing worktrees, confirm the current directory is inside the intended repository.

## Safety

- Treat `ofsht rm` as destructive. Confirm the exact target before removing a worktree unless the user explicitly named it in the current request.
- Do not remove the current worktree with `ofsht rm .` unless returning to the main worktree is the requested outcome.
- Avoid interactive `fzf` flows in unattended automation. Pass explicit branch names, paths, or `.`.
- Keep stdout clean when scripting around shell integration. `ofsht add`, `ofsht cd`, and some `ofsht rm` flows may print a path for wrapper functions.

## Core commands

Run `ofsht --help` or `ofsht <command> --help` when command details are uncertain.

- List worktrees:
  ```bash
  ofsht ls
  ofsht ls --show-path
  ```
- Create and navigate through shell integration:
  ```bash
  ofsht add feature-branch
  ofsht add feature-branch origin/main
  ```
- Create without navigation:
  ```bash
  ofsht create feature-branch
  ```
- Print a worktree path for shell integration:
  ```bash
  ofsht cd feature-branch
  ```
- Remove explicit worktrees:
  ```bash
  ofsht rm feature-branch
  ofsht rm /absolute/path/to/worktree
  ```
- Re-apply create hooks to existing worktrees:
  ```bash
  ofsht sync
  ofsht sync --run
  ofsht sync --copy
  ofsht sync --link
  ```
- Open other worktrees in tmux:
  ```bash
  ofsht open
  ofsht open --pane
  ```

## Configuration awareness

- Project config lives at `.ofsht.toml` in the main repository root.
- Worktree path templates and hooks are project settings.
- Integration settings for zoxide, fzf, tmux, and GitHub CLI behavior come from the user's global config, not project-local config.
- Hooks run after worktree creation and can execute commands, copy files, or create symlinks. Expect setup side effects such as dependency installation.

## Verification

After creating, removing, or syncing worktrees, verify the observable state instead of assuming success:

```bash
ofsht ls
git worktree list --porcelain
```

For creation tasks, check that the new worktree path exists and the expected branch is listed. For removal tasks, check that the target no longer appears in both `ofsht ls` and `git worktree list --porcelain`.
