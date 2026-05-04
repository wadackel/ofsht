#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Zoxide client interface for adding directories
pub trait ZoxideClient {
    fn add(&self, path: &Path) -> Result<()>;
}

/// Real zoxide implementation
#[derive(Debug, Default)]
pub struct RealZoxideClient;

impl ZoxideClient for RealZoxideClient {
    fn add(&self, path: &Path) -> Result<()> {
        let output = Command::new("zoxide")
            .arg("add")
            .arg(path)
            .output()
            .context("Failed to execute zoxide add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("zoxide add failed: {stderr}");
        }

        Ok(())
    }
}

/// Check if zoxide is available in the system
pub fn is_zoxide_available() -> bool {
    Command::new("zoxide")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct MockZoxideClient {
        should_fail: bool,
    }

    impl ZoxideClient for MockZoxideClient {
        fn add(&self, _path: &Path) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("Mock zoxide failure");
            }
            Ok(())
        }
    }

    #[test]
    fn test_mock_zoxide_client_success() {
        let client = MockZoxideClient { should_fail: false };
        let path = PathBuf::from("/test/path");
        let result = client.add(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_zoxide_client_failure() {
        let client = MockZoxideClient { should_fail: true };
        let path = PathBuf::from("/test/path");
        let result = client.add(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_zoxide_available() {
        // This test will pass or fail depending on whether zoxide is installed
        // We're just checking that the function doesn't panic
        let _ = is_zoxide_available();
    }
}
