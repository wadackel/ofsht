use anyhow::Result;
use std::path::{Path, PathBuf};

use super::session::{self, owning_worktree, Pane, Session};
use crate::path_utils::canonicalize_allow_missing;

struct Candidate {
    pane: Pane,
    worktree: PathBuf,
}

pub struct CleanupPlan {
    session: Session,
    roots: Vec<PathBuf>,
    candidates: Vec<Candidate>,
}

impl CleanupPlan {
    pub fn capture(roots: &[PathBuf], targets: &[&Path]) -> Result<Self> {
        let session = Session::current()?;
        session.validate_cleanup_scope()?;
        let roots: Vec<_> = roots
            .iter()
            .map(|root| canonicalize_allow_missing(root))
            .collect();
        let targets: Vec<_> = targets
            .iter()
            .map(|path| canonicalize_allow_missing(path))
            .collect();
        let mut candidates = Vec::new();
        for pane in session.panes()? {
            if pane.count != 1 {
                continue;
            }
            let Some(path) = &pane.path else {
                continue;
            };
            if !path.is_dir() {
                continue;
            }
            let Some(owner) = owning_worktree(path, &roots) else {
                continue;
            };
            if targets.contains(owner) {
                candidates.push(Candidate {
                    worktree: owner.clone(),
                    pane,
                });
            }
        }
        candidates.sort_by_key(|candidate| candidate.pane.window == session.window);
        Ok(Self {
            session,
            roots,
            candidates,
        })
    }

    pub fn print_dry_run(&self) {
        for candidate in &self.candidates {
            eprintln!(
                "Would close tmux window {} in {} after removing {}",
                candidate.pane.window,
                self.session.id,
                candidate.worktree.display()
            );
        }
    }

    pub fn finish(&self, removed: &[PathBuf]) {
        for candidate in &self.candidates {
            if !removed.contains(&candidate.worktree) {
                continue;
            }
            if let Err(error) = self.close_if_eligible(candidate) {
                eprintln!(
                    "Warning: tmux cleanup skipped for {}: {error:#}",
                    candidate.pane.window
                );
            }
        }
    }

    fn close_if_eligible(&self, candidate: &Candidate) -> Result<()> {
        self.session.validate_cleanup_scope()?;
        let panes = self.session.panes()?;
        let Some(pane) = panes
            .iter()
            .find(|pane| pane.id == candidate.pane.id && pane.window == candidate.pane.window)
        else {
            return Ok(());
        };
        if pane.count != 1 {
            return Ok(());
        }
        let Some(path) = pane.path_after_removal() else {
            return Ok(());
        };
        if owning_worktree(&path, &self.roots) != Some(&candidate.worktree) {
            return Ok(());
        }
        let target = format!("{}:{}", self.session.id, pane.window);
        let condition = format!("#{{&&:#{{&&:#{{==:#{{window_panes}},1}},#{{==:#{{session_grouped}},0}}}},#{{==:#{{pane_id}},{}}}}}", pane.id);
        // kill-window destroys independent links in other sessions; unlink only this session's link.
        let action = format!("unlink-window -k -t '{target}'");
        session::text(&["if-shell", "-F", "-t", &target, &condition, &action])?;
        Ok(())
    }
}
