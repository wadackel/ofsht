#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
mod client;
mod input;

pub use client::{GhClient, PrInfo, RealGhClient};
pub use input::BranchInput;

/// Build a branch name from an issue number
///
/// Format: `issue-{number}`
pub fn build_issue_branch(number: u32) -> String {
    format!("issue-{number}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_issue_branch() {
        assert_eq!(build_issue_branch(123), "issue-123");
    }

    #[test]
    fn test_build_issue_branch_single_digit() {
        assert_eq!(build_issue_branch(1), "issue-1");
    }

    #[test]
    fn test_build_issue_branch_large_number() {
        assert_eq!(build_issue_branch(99999), "issue-99999");
    }
}
