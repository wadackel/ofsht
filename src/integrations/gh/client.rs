#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

/// Information about a GitHub issue
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueInfo {
    pub number: u32,
    pub title: String,
    #[allow(dead_code)]
    pub url: String,
    /// Whether this is actually a pull request (GitHub treats PRs as special issues)
    pub is_pull_request: bool,
}

/// Information about a GitHub pull request
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub number: u32,
    pub title: String,
    #[allow(dead_code)]
    pub url: String,
    pub head_ref_name: String,
    /// Whether this PR is from a fork (cross-repository)
    pub is_cross_repository: bool,
}

/// Trait for interacting with GitHub CLI
pub trait GhClient {
    /// Get information about an issue
    fn issue_info(&self, number: u32) -> Result<IssueInfo>;

    /// Get information about a pull request
    fn pr_info(&self, number: u32) -> Result<PrInfo>;

    /// Check if gh CLI is available
    fn is_available(&self) -> bool;
}

/// Real implementation of `GhClient` using `gh` CLI
pub struct RealGhClient;

impl GhClient for RealGhClient {
    fn issue_info(&self, number: u32) -> Result<IssueInfo> {
        let output = Command::new("gh")
            .args([
                "issue",
                "view",
                &number.to_string(),
                "--json",
                "number,title,url,isPullRequest",
            ])
            .output()
            .context("Failed to execute gh command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh issue view failed: {stderr}");
        }

        let json = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse issue info JSON: {json}"))
    }

    fn pr_info(&self, number: u32) -> Result<PrInfo> {
        let output = Command::new("gh")
            .args([
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "number,title,url,headRefName,isCrossRepository",
            ])
            .output()
            .context("Failed to execute gh command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh pr view failed: {stderr}");
        }

        let json = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&json).with_context(|| format!("Failed to parse PR info JSON: {json}"))
    }

    fn is_available(&self) -> bool {
        Command::new("gh")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[derive(Clone)]
    enum MockResult<T> {
        Ok(T),
        Err(String),
    }

    /// Mock implementation for testing
    pub struct MockGhClient {
        issue_result: Option<MockResult<IssueInfo>>,
        pr_result: Option<MockResult<PrInfo>>,
        available: bool,
    }

    impl MockGhClient {
        pub fn new() -> Self {
            Self {
                issue_result: None,
                pr_result: None,
                available: true,
            }
        }

        pub fn with_issue(mut self, issue: IssueInfo) -> Self {
            self.issue_result = Some(MockResult::Ok(issue));
            self
        }

        pub fn with_pr(mut self, pr: PrInfo) -> Self {
            self.pr_result = Some(MockResult::Ok(pr));
            self
        }

        pub fn with_issue_error(mut self, error: &str) -> Self {
            self.issue_result = Some(MockResult::Err(error.to_string()));
            self
        }

        #[allow(dead_code)]
        pub fn with_pr_error(mut self, error: &str) -> Self {
            self.pr_result = Some(MockResult::Err(error.to_string()));
            self
        }

        pub fn unavailable(mut self) -> Self {
            self.available = false;
            self
        }
    }

    impl GhClient for MockGhClient {
        fn issue_info(&self, _number: u32) -> Result<IssueInfo> {
            match &self.issue_result {
                Some(MockResult::Ok(info)) => Ok(info.clone()),
                Some(MockResult::Err(msg)) => Err(anyhow::anyhow!("{msg}")),
                None => Err(anyhow::anyhow!("No issue result configured")),
            }
        }

        fn pr_info(&self, _number: u32) -> Result<PrInfo> {
            match &self.pr_result {
                Some(MockResult::Ok(info)) => Ok(info.clone()),
                Some(MockResult::Err(msg)) => Err(anyhow::anyhow!("{msg}")),
                None => Err(anyhow::anyhow!("No PR result configured")),
            }
        }

        fn is_available(&self) -> bool {
            self.available
        }
    }

    #[test]
    fn test_mock_client_with_issue() {
        let client = MockGhClient::new().with_issue(IssueInfo {
            number: 123,
            title: "Test issue".to_string(),
            url: "https://github.com/owner/repo/issues/123".to_string(),
            is_pull_request: false,
        });

        let info = client.issue_info(123).unwrap();
        assert_eq!(info.number, 123);
        assert_eq!(info.title, "Test issue");
        assert!(!info.is_pull_request);
    }

    #[test]
    fn test_mock_client_with_pr() {
        let client = MockGhClient::new().with_pr(PrInfo {
            number: 456,
            title: "Test PR".to_string(),
            url: "https://github.com/owner/repo/pull/456".to_string(),
            head_ref_name: "feature-branch".to_string(),
            is_cross_repository: false,
        });

        let info = client.pr_info(456).unwrap();
        assert_eq!(info.number, 456);
        assert_eq!(info.head_ref_name, "feature-branch");
        assert!(!info.is_cross_repository);
    }

    #[test]
    fn test_mock_client_with_issue_error() {
        let client = MockGhClient::new().with_issue_error("Not found");

        let result = client.issue_info(999);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Not found");
    }

    #[test]
    fn test_mock_client_unavailable() {
        let client = MockGhClient::new().unavailable();
        assert!(!client.is_available());
    }
}
