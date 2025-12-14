#![allow(clippy::literal_string_with_formatting_args)]
mod cli;
mod color;
mod commands;
mod config;
mod domain;
mod hooks;
mod integrations;
mod service;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::env::CompleteEnv;

// Use shared CLI definitions from cli module
use cli::{Cli, Commands};

#[cfg(test)]
use {
    chrono::{DateTime, Utc},
    commands::common::is_main_worktree,
    domain::worktree::{display_path, get_last_commit_time},
    std::path::PathBuf,
};

fn main() -> Result<()> {
    // Handle dynamic completion via COMPLETE environment variable
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();

    // Resolve color mode from CLI flag and environment variables
    let color_mode = color::ColorMode::resolve(cli.color);

    match cli.command {
        Commands::Add {
            branch,
            start_point,
            tmux,
            no_tmux,
        } => commands::add::cmd_new(&branch, start_point.as_deref(), tmux, no_tmux, color_mode),
        Commands::Create {
            branch,
            start_point,
        } => commands::create::cmd_create(&branch, start_point.as_deref(), color_mode),
        Commands::Ls { show_path } => commands::list::cmd_list(show_path, color_mode),
        Commands::Rm { targets } => commands::rm::cmd_rm_many(&targets, color_mode),
        Commands::Cd { name } => commands::cd::cmd_goto(name.as_deref(), color_mode),
        Commands::Init {
            global,
            local,
            force,
        } => commands::init::cmd_init(global, local, force, color_mode),
        Commands::Completion { shell } => commands::completion::cmd_completion(&shell),
        Commands::ShellInit { shell } => commands::shell_init::cmd_shell_init(&shell),
    }
}

// Re-export get_main_repo_root for backwards compatibility
pub use commands::common::get_main_repo_root;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{list_git_branches, list_git_worktrees, parse_worktree_list};
    use crate::commands::common::{canonicalize_allow_missing, find_worktree_by_branch};
    use crate::domain::worktree::{
        format_worktree_table, parse_simple_worktree_entries, parse_worktree_entries, WorktreeEntry,
    };

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn test_list_git_branches_returns_branches_in_git_repo() {
        // When running in a git repo, should return branch list (at least one branch exists)
        use std::ffi::OsStr;
        let result = list_git_branches(OsStr::new(""));
        // In a valid git repo, there should be at least one branch
        assert!(
            !result.is_empty(),
            "Should return branches when in git repo"
        );
    }

    #[test]
    fn test_list_git_branches_returns_completion_candidates() {
        // Verify that the returned values are valid CompletionCandidates
        use std::ffi::OsStr;
        let result = list_git_branches(OsStr::new(""));
        for candidate in result {
            // Each candidate should have non-empty value
            assert!(
                !candidate.get_value().is_empty(),
                "Branch name should not be empty"
            );
        }
    }

    #[test]
    fn test_list_git_branches_filters_by_prefix() {
        // Test that branches are filtered by prefix
        use std::ffi::OsStr;

        // Get all branches first
        let all_branches = list_git_branches(OsStr::new(""));

        if let Some(first_branch) = all_branches.first() {
            let branch_str = first_branch.get_value().to_string_lossy();
            if branch_str.len() >= 2 {
                let prefix = &branch_str[..2]; // Take first 2 characters as prefix
                let filtered = list_git_branches(OsStr::new(prefix));

                // All filtered branches should start with the prefix
                for candidate in &filtered {
                    let value = candidate.get_value().to_string_lossy();
                    assert!(
                        value.starts_with(prefix),
                        "Branch '{value}' should start with prefix '{prefix}'"
                    );
                }

                // Filtered list should be <= all branches
                assert!(filtered.len() <= all_branches.len());
            }
        }
    }

    #[test]
    fn test_parse_worktree_list_excludes_main() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        let result = parse_worktree_list(output);
        assert_eq!(result, vec!["feature"]);
    }

    #[test]
    fn test_parse_worktree_list_multiple_worktrees() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature-a
branch refs/heads/feature-a

worktree /path/to/feature-b
branch refs/heads/feature-b

";
        let result = parse_worktree_list(output);
        assert_eq!(result, vec!["feature-a", "feature-b"]);
    }

    #[test]
    fn test_parse_worktree_list_empty() {
        let output = "";
        let result = parse_worktree_list(output);
        assert!(result.is_empty());
    }

    #[test]
    fn test_display_path_under_home() {
        // Test path under home directory
        if let Some(home) = dirs::home_dir() {
            let test_path = home.join("test/path");
            let result = display_path(&test_path);
            assert!(result.starts_with("~/"));
            assert_eq!(result, "~/test/path");
        }
    }

    #[test]
    fn test_display_path_outside_home() {
        // Test path outside home directory
        let test_path = std::path::PathBuf::from("/tmp/test/path");
        let result = display_path(&test_path);
        assert_eq!(result, "/tmp/test/path");
    }

    #[test]
    fn test_display_path_home_itself() {
        // Test home directory itself
        if let Some(home) = dirs::home_dir() {
            let result = display_path(&home);
            assert_eq!(result, "~");
        }
    }

    #[test]
    fn test_find_worktree_by_branch_finds_path() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

worktree /path/to/bugfix
branch refs/heads/bugfix

";
        let result = find_worktree_by_branch(output, "feature");
        assert_eq!(result, Some("/path/to/feature".to_string()));
    }

    #[test]
    fn test_find_worktree_by_branch_not_found() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        let result = find_worktree_by_branch(output, "nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_worktree_by_branch_main_not_found() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        // Main worktree should not be found (it's excluded)
        let result = find_worktree_by_branch(output, "main");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_worktree_by_path_exact_match() {
        use commands::common::find_worktree_by_path;

        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

worktree /path/to/bugfix
branch refs/heads/bugfix

";
        let result = find_worktree_by_path(output, &PathBuf::from("/path/to/feature"));
        assert_eq!(result, Some("/path/to/feature".to_string()));
    }

    #[test]
    fn test_find_worktree_by_path_not_found() {
        use commands::common::find_worktree_by_path;

        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        let result = find_worktree_by_path(output, &PathBuf::from("/path/to/nonexistent"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_worktree_by_path_main_excluded() {
        use commands::common::find_worktree_by_path;

        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        // Main worktree should not be found (it's excluded)
        let result = find_worktree_by_path(output, &PathBuf::from("/path/to/main"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_is_main_worktree_by_path() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        assert!(is_main_worktree(output, "/path/to/main"));
        assert!(!is_main_worktree(output, "/path/to/feature"));
        assert!(!is_main_worktree(output, "/nonexistent"));
    }

    #[test]
    fn test_is_main_worktree_by_branch() {
        let output = "worktree /path/to/main
branch refs/heads/develop

worktree /path/to/feature
branch refs/heads/feature

";
        // "@" is always treated as main worktree
        assert!(is_main_worktree(output, "@"));
        // Branch name matching main worktree's branch is also considered main
        assert!(is_main_worktree(output, "develop"));
        assert!(!is_main_worktree(output, "feature"));
    }

    #[test]
    fn test_parse_worktree_entries_single_worktree() {
        let output = "worktree /path/to/main
HEAD 1234567890abcdef1234567890abcdef
branch refs/heads/main

";
        let result = parse_worktree_entries(output, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, Some("main".to_string()));
        // Hash should be first 8 chars of HEAD
        assert_eq!(result[0].hash, "12345678");
    }

    #[test]
    fn test_parse_worktree_entries_multiple_worktrees() {
        let output = "worktree /path/to/main
HEAD 1234567890abcdef1234567890abcdef
branch refs/heads/main

worktree /path/to/feature
HEAD abcdef1234567890abcdef1234567890
branch refs/heads/feature

worktree /path/to/bugfix
HEAD fedcba0987654321fedcba0987654321
branch refs/heads/bugfix

";
        let result = parse_worktree_entries(output, None);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, Some("main".to_string()));
        assert_eq!(result[0].hash, "12345678");

        assert_eq!(result[1].path, "/path/to/feature");
        assert_eq!(result[1].branch, Some("feature".to_string()));
        assert_eq!(result[1].hash, "abcdef12");

        assert_eq!(result[2].path, "/path/to/bugfix");
        assert_eq!(result[2].branch, Some("bugfix".to_string()));
        assert_eq!(result[2].hash, "fedcba09");
    }

    #[test]
    fn test_parse_worktree_entries_detached_head() {
        let output = "worktree /path/to/main
HEAD 1234567890abcdef1234567890abcdef
detached

worktree /path/to/feature
HEAD abcdef1234567890abcdef1234567890
branch refs/heads/feature

";
        let result = parse_worktree_entries(output, None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, None);
        assert_eq!(result[0].hash, "12345678");

        assert_eq!(result[1].path, "/path/to/feature");
        assert_eq!(result[1].branch, Some("feature".to_string()));
        assert_eq!(result[1].hash, "abcdef12");
    }

    #[test]
    fn test_parse_worktree_entries_empty() {
        let output = "";
        let result = parse_worktree_entries(output, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_worktree_entries_with_active_path() {
        let output = "worktree /path/to/main
HEAD 1234567890abcdef1234567890abcdef
branch refs/heads/main

worktree /path/to/feature
HEAD abcdef1234567890abcdef1234567890
branch refs/heads/feature

";
        let active_path = std::path::PathBuf::from("/path/to/feature");
        let result = parse_worktree_entries(output, Some(&active_path));

        assert_eq!(result.len(), 2);
        assert!(!result[0].is_active);
        assert!(result[1].is_active);
    }

    #[test]
    fn test_parse_worktree_entries_without_active_path() {
        let output = "worktree /path/to/main
HEAD 1234567890abcdef1234567890abcdef
branch refs/heads/main

worktree /path/to/feature
HEAD abcdef1234567890abcdef1234567890
branch refs/heads/feature

";
        let result = parse_worktree_entries(output, None);

        assert_eq!(result.len(), 2);
        assert!(!result[0].is_active);
        assert!(!result[1].is_active);
    }

    #[test]
    fn test_get_last_commit_time_current_repo() {
        // Test with current repository (should have commits)
        let current_dir = std::env::current_dir().unwrap();
        let result = get_last_commit_time(&current_dir);
        // Current repo should have commits
        assert!(result.is_some(), "Current repository should have commits");
    }

    #[test]
    fn test_get_last_commit_time_nonexistent_path() {
        // Test with non-existent path (should return None)
        let nonexistent = std::path::PathBuf::from("/nonexistent/path/to/worktree");
        let result = get_last_commit_time(&nonexistent);
        assert!(result.is_none(), "Non-existent path should return None");
    }

    #[test]
    fn test_format_worktree_table_default_no_path() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/main".to_string(),
            branch: Some("main".to_string()),
            hash: "a1b2c3d4".to_string(),
            is_active: false,
        }];
        let commit_times = vec![Some(
            DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap(),
        )];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 1);
        // Line should contain hash, branch, and timestamp (no path)
        assert!(!result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
    }

    #[test]
    fn test_format_worktree_table_with_path() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/main".to_string(),
            branch: Some("main".to_string()),
            hash: "a1b2c3d4".to_string(),
            is_active: false,
        }];
        let commit_times = vec![Some(
            DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap(),
        )];

        let result =
            format_worktree_table(&entries, &commit_times, true, color::ColorMode::Never, None);
        assert_eq!(result.len(), 1);
        // Line should contain path, hash, branch, and timestamp
        assert!(result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
    }

    #[test]
    fn test_format_worktree_table_multiple_entries() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: "a1b2c3d4".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature-branch".to_string(),
                branch: Some("feature".to_string()),
                hash: "e5f6g7h8".to_string(),
                is_active: false,
            },
        ];
        let commit_times = vec![
            Some(DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap()),
            None,
        ];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);
        // Both lines should have same structure (no paths)
        assert!(!result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
        assert!(!result[1].contains("/path/to/feature-branch"));
        assert!(result[1].contains("[feature]"));
        assert!(result[1].contains("e5f6g7h8"));
        assert!(result[1].contains("–")); // No commit time
    }

    #[test]
    fn test_format_worktree_table_column_alignment() {
        let entries = vec![
            WorktreeEntry {
                path: "/short".to_string(),
                branch: Some("a".to_string()),
                hash: "12345678".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/very/long/path/to/worktree".to_string(),
                branch: Some("feature-branch".to_string()),
                hash: "abcdefgh".to_string(),
                is_active: false,
            },
        ];
        let commit_times = vec![None, None];

        let result =
            format_worktree_table(&entries, &commit_times, true, color::ColorMode::Never, None);
        assert_eq!(result.len(), 2);

        // Verify that both lines contain the expected content
        assert!(result[0].contains("/short"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("12345678"));
        assert!(result[1].contains("/very/long/path/to/worktree"));
        assert!(result[1].contains("[feature-branch]"));
        assert!(result[1].contains("abcdefgh"));

        // Both lines should end with the same timestamp placeholder
        assert!(result[0].ends_with("–"));
        assert!(result[1].ends_with("–"));
    }

    #[test]
    fn test_format_worktree_table_detached_head() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/detached".to_string(),
            branch: None,
            hash: "deadbeef".to_string(),
            is_active: false,
        }];
        let commit_times = vec![None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 1);
        // Main worktree (first entry) is always [@], even if detached
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("deadbeef"));
        assert!(result[0].contains("–"));
    }

    #[test]
    fn test_format_worktree_table_active_marker() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: "a1b2c3d4".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: "e5f6g7h8".to_string(),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);
        // First entry (inactive) should have space prefix
        assert!(result[0].starts_with("  "));
        // Second entry (active) should have * prefix
        assert!(result[1].starts_with("* "));
        assert!(result[1].contains("[feature]"));
    }

    #[test]
    fn test_format_worktree_table_active_marker_with_path() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: "a1b2c3d4".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: "e5f6g7h8".to_string(),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None];

        let result =
            format_worktree_table(&entries, &commit_times, true, color::ColorMode::Never, None);
        assert_eq!(result.len(), 2);
        // Both entries should have marker prefix (space or *)
        assert!(result[0].starts_with("  "));
        assert!(result[1].starts_with("* "));
    }

    #[test]
    fn test_format_worktree_table_with_relative_paths() {
        // Test that relative paths are displayed when config is provided
        use config::{Config, Hooks, IntegrationsConfig, WorktreeConfig};

        let entries = vec![
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/main".to_string(),
                branch: Some("main".to_string()),
                hash: "a1b2c3d4".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: "e5f6g7h8".to_string(),
                is_active: false,
            },
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/docs/tweak".to_string(),
                branch: Some("docs/tweak".to_string()),
                hash: "i9j0k1l2".to_string(),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None, None];

        let config = Config {
            worktree: WorktreeConfig {
                dir: "../{repo}-worktrees/{branch}".to_string(),
            },
            hooks: Hooks::default(),
            integrations: IntegrationsConfig::default(),
        };

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            color::ColorMode::Never,
            Some(&config),
        );

        assert_eq!(result.len(), 3);

        // Main worktree (index 0) should have blank relative path column
        // Format: "  a1b2c3d4            [@]  unknown" (extra spaces where rel_path would be)
        let main_line = &result[0];
        assert!(main_line.contains("a1b2c3d4"));
        assert!(main_line.contains("[@]"));

        // Feature worktree should show "feature" as relative path
        // Format: "  e5f6g7h8  feature  [feature]  unknown"
        let feature_line = &result[1];
        assert!(feature_line.contains("e5f6g7h8"));
        assert!(feature_line.contains("feature"));
        assert!(feature_line.contains("[feature]"));

        // Nested worktree should show "docs/tweak" as relative path
        // Format: "* i9j0k1l2  docs/tweak  [docs/tweak]  unknown"
        let nested_line = &result[2];
        assert!(nested_line.contains("i9j0k1l2"));
        assert!(nested_line.contains("docs/tweak"));
        assert!(nested_line.contains("[docs/tweak]"));
        assert!(nested_line.starts_with("* ")); // Active marker
    }

    #[test]
    fn test_list_git_worktrees_includes_at_symbol() {
        // Test that @ is included in worktree completion candidates
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new(""));
        // Should include @ as the first candidate (or at least include it)
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(has_at, "Completion candidates should include @");
    }

    #[test]
    fn test_list_git_worktrees_filters_at_symbol() {
        // Test that @ is filtered correctly by prefix
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new("@"));
        // Should include @ when prefix is @
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(
            has_at,
            "Completion candidates should include @ when prefix is @"
        );
    }

    #[test]
    fn test_list_git_worktrees_excludes_at_with_different_prefix() {
        // Test that @ is excluded when prefix doesn't match
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new("feature"));
        // Should not include @ when prefix is "feature"
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(
            !has_at,
            "Completion candidates should not include @ when prefix is 'feature'"
        );
    }

    // Tests for parse_simple_worktree_entries
    #[test]
    fn test_parse_simple_worktree_entries_single() {
        let output = "worktree /path/to/main
branch refs/heads/main

";
        let result = parse_simple_worktree_entries(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, Some("main".to_string()));
    }

    #[test]
    fn test_parse_simple_worktree_entries_multiple() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

worktree /path/to/bugfix
branch refs/heads/bugfix

";
        let result = parse_simple_worktree_entries(output);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, Some("main".to_string()));
        assert_eq!(result[1].path, "/path/to/feature");
        assert_eq!(result[1].branch, Some("feature".to_string()));
        assert_eq!(result[2].path, "/path/to/bugfix");
        assert_eq!(result[2].branch, Some("bugfix".to_string()));
    }

    #[test]
    fn test_parse_simple_worktree_entries_detached() {
        let output = "worktree /path/to/main
detached

worktree /path/to/feature
branch refs/heads/feature

";
        let result = parse_simple_worktree_entries(output);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, None);
        assert_eq!(result[1].path, "/path/to/feature");
        assert_eq!(result[1].branch, Some("feature".to_string()));
    }

    #[test]
    fn test_parse_simple_worktree_entries_empty() {
        let output = "";
        let result = parse_simple_worktree_entries(output);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_simple_worktree_entries_no_blank_lines() {
        // Test without trailing blank lines
        let output = "worktree /path/to/main
branch refs/heads/main";
        let result = parse_simple_worktree_entries(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/path/to/main");
        assert_eq!(result[0].branch, Some("main".to_string()));
    }

    // Tests for canonicalize_allow_missing
    #[test]
    fn test_canonicalize_allow_missing_existing_path() {
        // Test with existing path (current directory)
        let current_dir = std::env::current_dir().unwrap();
        let result = canonicalize_allow_missing(&current_dir);
        // Should return canonicalized path
        assert_eq!(result, current_dir.canonicalize().unwrap());
    }

    #[test]
    fn test_canonicalize_allow_missing_nonexistent_absolute() {
        // Test with nonexistent absolute path
        let nonexistent = std::path::PathBuf::from("/nonexistent/path/to/worktree");
        let result = canonicalize_allow_missing(&nonexistent);
        // Should return the absolute path (may have symlinks resolved in existing parts)
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_relative() {
        // Test with relative path
        let relative = std::path::PathBuf::from("./test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
    }

    #[test]
    fn test_canonicalize_allow_missing_relative_nonexistent() {
        // Test with nonexistent relative path
        let relative = std::path::PathBuf::from("./nonexistent/test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_deep_nonexistent() {
        // Test with deeply nested nonexistent path
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let deep_path = current_dir.join("a/b/c/d/e/f/nonexistent");
        let result = canonicalize_allow_missing(&deep_path);
        // Should start with canonicalized current dir (not symlinked version)
        assert!(
            result.starts_with(&current_dir),
            "Result {} should start with canonicalized current_dir {}",
            result.display(),
            current_dir.display()
        );
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_parent_dots() {
        // Test with parent directory references
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let with_dots = current_dir.join("foo/../bar/./baz");
        let result = canonicalize_allow_missing(&with_dots);
        // Should resolve . and .. references even when path doesn't exist
        let expected = current_dir.join("bar/baz");
        assert_eq!(
            result, expected,
            "Should normalize .. and . even in nonexistent paths"
        );
        // Result should not contain literal . or .. components
        let result_str = result.to_string_lossy();
        assert!(
            !result_str.contains("/./"),
            "Result should not contain /./: {result_str}"
        );
        assert!(
            !result_str.contains("/../"),
            "Result should not contain /../: {result_str}"
        );
    }
}
