use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Rebase,
    Merge,
    Bisect,
    Am,
    CherryPick,
    Revert,
}

impl Operation {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rebase => "rebasing",
            Self::Merge => "merging",
            Self::Bisect => "bisecting",
            Self::Am => "applying patches",
            Self::CherryPick => "cherry-picking",
            Self::Revert => "reverting",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeState {
    pub locked: Option<String>,
    pub prunable: Option<String>,
    pub operation: Option<Operation>,
    pub operation_branch: Option<String>,
    pub missing: bool,
    pub dirty: Option<bool>,
    pub upstream: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub error: Option<String>,
}

impl WorktreeState {
    #[must_use]
    pub fn markers(&self) -> String {
        let mut markers = Vec::new();
        if self.locked.is_some() {
            markers.push("locked".to_string());
        }
        if self.prunable.is_some() {
            markers.push("prunable".to_string());
        }
        if self.missing {
            markers.push("missing".to_string());
        }
        if let Some(dirty) = self.dirty {
            markers.push(if dirty { "dirty" } else { "clean" }.to_string());
        }
        if let Some(ahead) = self.ahead.filter(|n| *n > 0) {
            markers.push(format!("+{ahead}"));
        }
        if let Some(behind) = self.behind.filter(|n| *n > 0) {
            markers.push(format!("-{behind}"));
        }
        if self.error.is_some() {
            markers.push("unknown".to_string());
        }
        markers.join(" ")
    }
}
