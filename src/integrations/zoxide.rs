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
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Add a path to zoxide if enabled and available
///
/// This function gracefully handles the case where zoxide is not installed.
/// It will only return an error if zoxide is installed but fails to add the path.
pub fn add_to_zoxide_if_enabled(path: &Path, enabled: bool) -> Result<()> {
    if !enabled {
        return Ok(());
    }

    if !is_zoxide_available() {
        // Gracefully skip if zoxide is not installed
        return Ok(());
    }

    let client = RealZoxideClient;
    client.add(path)
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

    #[test]
    fn test_add_to_zoxide_if_enabled_disabled() {
        let path = PathBuf::from("/test/path");
        let result = add_to_zoxide_if_enabled(&path, false);
        // Should succeed even if zoxide is not available
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_to_zoxide_if_enabled_enabled() {
        // This test will succeed if:
        // 1. zoxide is installed and can add the path (success)
        // 2. zoxide is not installed (gracefully skipped, success)
        // It may fail if zoxide is installed but returns an error
        let path = PathBuf::from("/tmp");
        let result = add_to_zoxide_if_enabled(&path, true);

        if is_zoxide_available() {
            // If zoxide is available, it should successfully add the path
            assert!(result.is_ok(), "Failed to add to zoxide: {result:?}");
        } else {
            // If zoxide is not available, it should gracefully succeed
            assert!(result.is_ok());
        }
    }
}
