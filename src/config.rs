//! Configuration module
//!
//! This module handles loading and managing ofsht configuration from TOML files.

pub mod loader;
pub mod schema;

// Re-export public types and functions
// Note: These are part of the public API and used in tests, even if not all are used in main.rs
#[allow(unused_imports)]
pub use schema::{
    Config, FzfConfig, GhConfig, HookActions, Hooks, IntegrationsConfig, TmuxBehavior, TmuxConfig,
    WorktreeConfig, ZoxideConfig,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.worktree.dir, "../{repo}-worktrees/{branch}");
        assert!(config.hooks.create.run.is_empty());
        assert!(config.hooks.delete.run.is_empty());
        assert!(config.integrations.zoxide.enabled);
        assert!(config.integrations.fzf.enabled);
        assert_eq!(config.integrations.tmux.create, "window");
    }

    #[test]
    fn test_zoxide_config_default() {
        let config = ZoxideConfig::default();
        assert!(config.enabled);
    }

    #[test]
    fn test_zoxide_config_from_toml() {
        let toml = r"
            [integration.zoxide]
            enabled = false
        ";
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.integrations.zoxide.enabled);
    }

    #[test]
    fn test_zoxide_config_missing_defaults_to_true() {
        let toml = r#"
            [worktree]
            dir = "/tmp/worktrees/{branch}"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.integrations.zoxide.enabled);
    }

    #[test]
    fn test_fzf_config_default() {
        let config = FzfConfig::default();
        assert!(config.enabled);
        assert!(config.options.is_empty());
    }

    #[test]
    fn test_fzf_config_from_toml() {
        let toml = r#"
            [integration.fzf]
            enabled = false
            options = ["--height=50%", "--border"]
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.integrations.fzf.enabled);
        assert_eq!(config.integrations.fzf.options.len(), 2);
    }

    #[test]
    fn test_tmux_config_default() {
        let config = TmuxConfig::default();
        assert_eq!(config.behavior, TmuxBehavior::Auto);
        assert_eq!(config.create, "window");
    }

    #[test]
    fn test_tmux_config_from_toml() {
        let toml = r#"
            [integration.tmux]
            behavior = "always"
            create = "pane"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Always);
        assert_eq!(config.integrations.tmux.create, "pane");
    }

    #[test]
    fn test_tmux_config_missing_defaults_to_window() {
        let toml = r#"
            [integration.tmux]
            behavior = "never"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.create, "window");
    }

    #[test]
    fn test_tmux_config_behavior_auto() {
        let toml = r#"
            [integration.tmux]
            behavior = "auto"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Auto);
    }

    #[test]
    fn test_tmux_config_behavior_always() {
        let toml = r#"
            [integration.tmux]
            behavior = "always"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Always);
    }

    #[test]
    fn test_tmux_config_behavior_never() {
        let toml = r#"
            [integration.tmux]
            behavior = "never"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Never);
    }

    #[test]
    fn test_tmux_behavior_invalid_value() {
        let toml = r#"
            [integration.tmux]
            behavior = "invalid"
        "#;
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_gh_config_default() {
        let config = GhConfig::default();
        assert!(config.enabled);
    }

    #[test]
    fn test_gh_config_from_toml() {
        let toml = r"
            [integration.gh]
            enabled = false
        ";
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.integrations.gh.enabled);
    }

    #[test]
    fn test_gh_config_enabled_by_default() {
        let toml = r#"
            [worktree]
            dir = "/tmp"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.integrations.gh.enabled);
    }

    #[test]
    fn test_integrations_default() {
        let config = IntegrationsConfig::default();
        assert!(config.zoxide.enabled);
        assert!(config.fzf.enabled);
        assert_eq!(config.tmux.behavior, TmuxBehavior::Auto);
        assert_eq!(config.tmux.create, "window");
        assert!(config.gh.enabled);
    }

    #[test]
    fn test_integrations_config_from_toml() {
        let toml = r#"
            [integration.zoxide]
            enabled = false

            [integration.fzf]
            enabled = true
            options = ["--height=100%"]

            [integration.tmux]
            behavior = "always"
            create = "pane"

            [integration.gh]
            enabled = true
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.integrations.zoxide.enabled);
        assert!(config.integrations.fzf.enabled);
        assert_eq!(config.integrations.fzf.options, vec!["--height=100%"]);
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Always);
        assert_eq!(config.integrations.tmux.create, "pane");
        assert!(config.integrations.gh.enabled);
    }

    #[test]
    fn test_local_config_path() {
        let path = Config::local_config_path();
        assert_eq!(path, std::path::PathBuf::from(".ofsht.toml"));
    }

    #[test]
    fn test_local_config_path_from_repo_root() {
        let repo_root = std::path::PathBuf::from("/tmp/my-repo");
        let path = Config::local_config_path_from(&repo_root);
        assert_eq!(path, std::path::PathBuf::from("/tmp/my-repo/.ofsht.toml"));
    }

    #[test]
    fn test_global_config_path_is_public() {
        // This test just ensures the method is public
        let _ = Config::global_config_path();
    }

    #[test]
    #[serial_test::serial]
    fn test_global_config_path_default() {
        // Clear XDG_CONFIG_HOME to test default behavior
        std::env::remove_var("XDG_CONFIG_HOME");
        if let Some(path) = Config::global_config_path() {
            assert!(path.ends_with(".config/ofsht/config.toml"));
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_global_config_path_with_xdg_env() {
        // Set XDG_CONFIG_HOME to an absolute path
        let xdg_path = std::env::temp_dir().join("xdg_config");
        std::env::set_var("XDG_CONFIG_HOME", &xdg_path);

        let path = Config::global_config_path();
        assert_eq!(path, Some(xdg_path.join("ofsht/config.toml")));

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    #[serial_test::serial]
    fn test_global_config_path_relative_xdg_ignored() {
        // Set XDG_CONFIG_HOME to a relative path (should be ignored)
        std::env::set_var("XDG_CONFIG_HOME", "relative/path");

        if let Some(path) = Config::global_config_path() {
            // Should fall back to default (~/.config/ofsht/config.toml)
            assert!(path.ends_with(".config/ofsht/config.toml"));
        }

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn test_load_from_repo_root_fallback_to_global() {
        // Create a temporary repo root without .ofsht.toml
        let temp_dir = std::env::temp_dir().join("ofsht_test_repo");
        std::fs::create_dir_all(&temp_dir).ok();

        // Should fall back to global or default config
        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_ok());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_from_specific_repo_root() {
        // Create a temporary repo root with .ofsht.toml
        let temp_dir = std::env::temp_dir().join("ofsht_test_repo_specific");
        std::fs::create_dir_all(&temp_dir).ok();

        let config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &config_path,
            r#"
                [worktree]
                dir = "/tmp/custom/{branch}"
            "#,
        )
        .ok();

        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().worktree.dir, "/tmp/custom/{branch}");

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_merge_configs() {
        let base = Config {
            worktree: WorktreeConfig {
                dir: "/base/{branch}".to_string(),
            },
            hooks: Hooks::default(),
            integrations: IntegrationsConfig::default(),
        };

        let override_config = Config {
            worktree: WorktreeConfig {
                dir: "/override/{branch}".to_string(),
            },
            hooks: Hooks::default(),
            integrations: IntegrationsConfig::default(),
        };

        let merged = base.merge(&override_config);
        assert_eq!(merged.worktree.dir, "/override/{branch}");
    }

    #[test]
    fn test_local_config_ignores_integrations() {
        let temp_dir = std::env::temp_dir().join("ofsht_test_local_integrations");
        std::fs::create_dir_all(&temp_dir).ok();

        let config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &config_path,
            r#"
                [worktree]
                dir = "/tmp/{branch}"

                [integration.zoxide]
                enabled = false

                [integration.fzf]
                enabled = false
            "#,
        )
        .ok();

        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_ok());
        let config = result.unwrap();

        // Worktree config should be loaded from local
        assert_eq!(config.worktree.dir, "/tmp/{branch}");

        // Integration config should be from global (or defaults)
        // Since we don't have a global config in test, it should use defaults
        assert!(config.integrations.zoxide.enabled); // Default is true
        assert!(config.integrations.fzf.enabled); // Default is true

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_local_config_uses_global_integration_settings() {
        let temp_dir = std::env::temp_dir().join("ofsht_test_integration_override");
        std::fs::create_dir_all(&temp_dir).ok();

        let local_config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &local_config_path,
            r#"
                [worktree]
                dir = "/local/{branch}"

                [integration.zoxide]
                enabled = false
            "#,
        )
        .ok();

        let config = Config::load_from_repo_root(&temp_dir).unwrap();

        // Local worktree settings should be used
        assert_eq!(config.worktree.dir, "/local/{branch}");

        // Integration settings should be ignored from local config
        // and should use global config or defaults
        assert!(config.integrations.zoxide.enabled); // Default

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn test_local_config_without_global_uses_defaults() {
        // Temporarily override XDG_CONFIG_HOME to a non-existent path
        let fake_xdg = std::env::temp_dir().join("fake_xdg_no_global");
        std::env::set_var("XDG_CONFIG_HOME", &fake_xdg);

        let temp_dir = std::env::temp_dir().join("ofsht_test_no_global");
        std::fs::create_dir_all(&temp_dir).ok();

        let local_config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &local_config_path,
            r#"
                [worktree]
                dir = "/no-global/{branch}"
            "#,
        )
        .ok();

        let config = Config::load_from_repo_root(&temp_dir).unwrap();

        // Local worktree settings
        assert_eq!(config.worktree.dir, "/no-global/{branch}");

        // Integration settings should use defaults (no global config)
        assert!(config.integrations.zoxide.enabled);
        assert!(config.integrations.fzf.enabled);

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_local_config_ignores_zoxide() {
        let temp_dir = std::env::temp_dir().join("ofsht_test_zoxide_ignore");
        std::fs::create_dir_all(&temp_dir).ok();

        let config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &config_path,
            r"
                [integration.zoxide]
                enabled = false
            ",
        )
        .ok();

        let config = Config::load_from_repo_root(&temp_dir).unwrap();
        // Should use global or default (true), not local setting
        assert!(config.integrations.zoxide.enabled);

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_local_config_ignores_gh() {
        let temp_dir = std::env::temp_dir().join("ofsht_test_gh_ignore");
        std::fs::create_dir_all(&temp_dir).ok();

        let config_path = temp_dir.join(".ofsht.toml");
        std::fs::write(
            &config_path,
            r"
                [integration.gh]
                enabled = false
            ",
        )
        .ok();

        let config = Config::load_from_repo_root(&temp_dir).unwrap();
        // Should use global or default (true), not local setting
        assert!(config.integrations.gh.enabled);

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_template_global_is_valid_toml() {
        let template = Config::template_global();
        let result: Result<Config, _> = toml::from_str(template);
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_local_is_valid_toml() {
        let template = Config::template_local();
        let result: Result<Config, _> = toml::from_str(template);
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_global_contains_all_sections() {
        let template = Config::template_global();
        assert!(template.contains("[worktree]"));
        assert!(template.contains("[hooks.create]"));
        assert!(template.contains("[hooks.delete]"));
        assert!(template.contains("[integration.zoxide]"));
        assert!(template.contains("[integration.fzf]"));
        assert!(template.contains("[integration.tmux]"));
        assert!(template.contains("[integration.gh]"));
    }

    #[test]
    fn test_template_local_contains_all_sections() {
        let template = Config::template_local();
        assert!(template.contains("[worktree]"));
        assert!(template.contains("[hooks.create]"));
        assert!(template.contains("[hooks.delete]"));
        // Should NOT contain integration sections
        assert!(!template.contains("[integration.zoxide]"));
        assert!(!template.contains("[integration.fzf]"));
        assert!(!template.contains("[integration.tmux]"));
        assert!(!template.contains("[integration.gh]"));
    }

    #[test]
    fn test_template_global_has_explanatory_comments() {
        let template = Config::template_global();
        assert!(template.contains("# Location:"));
        assert!(template.contains("# Variables:"));
    }
}
