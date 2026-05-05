//! Configuration module
//!
//! This module handles loading and managing ofsht configuration from TOML files.

pub mod loader;
pub mod schema;
pub mod template_generator;

// Re-export public types and functions
// Note: These are part of the public API and used in tests, even if not all are used in main.rs
#[allow(unused_imports)]
pub use schema::{
    Config, FzfConfig, GhConfig, HookActions, Hooks, IntegrationsConfig, OpenMode, ProjectConfig,
    TmuxBehavior, TmuxConfig, UserConfig, WorktreeConfig, ZoxideConfig,
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
        assert_eq!(config.integrations.tmux.create, OpenMode::Window);
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
        assert_eq!(config.create, OpenMode::Window);
        assert_eq!(config.open, OpenMode::Window);
    }

    #[test]
    fn test_tmux_config_from_toml() {
        let toml = r#"
            [integration.tmux]
            behavior = "always"
            create = "pane"
            open = "pane"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Always);
        assert_eq!(config.integrations.tmux.create, OpenMode::Pane);
        assert_eq!(config.integrations.tmux.open, OpenMode::Pane);
    }

    #[test]
    fn test_tmux_config_missing_defaults_to_window() {
        let toml = r#"
            [integration.tmux]
            behavior = "never"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.integrations.tmux.create, OpenMode::Window);
        assert_eq!(config.integrations.tmux.open, OpenMode::Window);
    }

    #[test]
    fn test_tmux_create_invalid_value() {
        let toml = r#"
            [integration.tmux]
            create = "invalid"
        "#;
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_tmux_open_invalid_value() {
        let toml = r#"
            [integration.tmux]
            open = "invalid"
        "#;
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_open_mode_display() {
        assert_eq!(format!("{}", OpenMode::Window), "window");
        assert_eq!(format!("{}", OpenMode::Pane), "pane");
    }

    #[test]
    fn test_open_mode_default() {
        assert_eq!(OpenMode::default(), OpenMode::Window);
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
        assert_eq!(config.tmux.create, OpenMode::Window);
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
        assert_eq!(config.integrations.tmux.create, OpenMode::Pane);
        assert!(config.integrations.gh.enabled);
    }

    // ---- ProjectConfig / UserConfig parse-only DTO unit tests ----

    #[test]
    fn test_project_config_accepts_hooks_and_worktree() {
        let toml = r#"
            [worktree]
            dir = "/tmp/{branch}"

            [hooks.create]
            run = ["echo hi"]

            [hooks.delete]
            run = ["echo bye"]
        "#;
        let parsed: ProjectConfig = toml::from_str(toml).expect("project-only TOML must parse");
        assert_eq!(parsed.worktree.dir, "/tmp/{branch}");
        assert_eq!(parsed.hooks.create.run, vec!["echo hi"]);
        assert_eq!(parsed.hooks.delete.run, vec!["echo bye"]);
    }

    #[test]
    fn test_project_config_denies_integration_section() {
        let toml = r#"
            [worktree]
            dir = "/tmp/{branch}"

            [integration.zoxide]
            enabled = false
        "#;
        let result: Result<ProjectConfig, _> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "ProjectConfig must reject [integration.*] sections"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unknown field `integration`"),
            "expected unknown-field error, got: {msg}"
        );
    }

    #[test]
    fn test_user_config_accepts_integration_alias() {
        let toml = r#"
            [worktree]
            dir = "/tmp/{branch}"

            [integration.tmux]
            behavior = "always"
            create = "pane"
            open = "pane"
        "#;
        let parsed: UserConfig =
            toml::from_str(toml).expect("user TOML with integration alias must parse");
        assert_eq!(parsed.integrations.tmux.behavior, TmuxBehavior::Always);
        assert_eq!(parsed.integrations.tmux.create, OpenMode::Pane);
        assert_eq!(parsed.integrations.tmux.open, OpenMode::Pane);
    }

    #[test]
    fn test_user_config_denies_unknown_top_level_field() {
        let toml = r#"
            [wortkree]
            dir = "/typo"
        "#;
        let result: Result<UserConfig, _> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "UserConfig must reject unknown top-level fields"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unknown field `wortkree`"),
            "expected unknown-field error, got: {msg}"
        );
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
    #[serial_test::serial]
    fn test_load_from_repo_root_fallback_to_global() {
        // Serial: load_from_repo_root reads the global config via
        // `XDG_CONFIG_HOME`, which other tests mutate.
        let temp_dir = std::env::temp_dir().join("ofsht_test_repo");
        std::fs::create_dir_all(&temp_dir).ok();

        // Should fall back to global or default config
        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_ok());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    #[serial_test::serial]
    fn test_load_from_specific_repo_root() {
        // Serial: same env-state dependency as the fallback test above.
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
    fn test_local_config_with_integration_section_fails_to_load() {
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

        // ProjectConfig denies `integration` at the top level via
        // `#[serde(deny_unknown_fields)]`. Loading must fail with a parse
        // error mentioning the offending field.
        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("unknown field `integration`"),
            "expected unknown-field error, got: {msg}"
        );

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_local_config_with_integration_section_fails_even_with_other_fields() {
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

        // Even when paired with valid `[worktree]` and `[hooks.*]`, an
        // `[integration.*]` section in a project-local file is rejected.
        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("unknown field `integration`"),
            "expected unknown-field error, got: {msg}"
        );

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

    // ---- build_effective_config composition matrix ----
    // ¬project ∧ ¬user is covered by `test_load_from_repo_root_fallback_to_global` above.

    #[test]
    #[serial_test::serial]
    fn test_compose_project_and_user_uses_user_integrations() {
        let xdg = std::env::temp_dir().join("ofsht_test_compose_both_xdg");
        let global_dir = xdg.join("ofsht");
        std::fs::create_dir_all(&global_dir).ok();
        std::fs::write(
            global_dir.join("config.toml"),
            r#"
                [worktree]
                dir = "/global/{branch}"

                [integration.zoxide]
                enabled = false

                [integration.fzf]
                enabled = false
            "#,
        )
        .ok();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        let repo_root = std::env::temp_dir().join("ofsht_test_compose_both_repo");
        std::fs::create_dir_all(&repo_root).ok();
        std::fs::write(
            repo_root.join(".ofsht.toml"),
            r#"
                [worktree]
                dir = "/local/{branch}"

                [hooks.create]
                run = ["echo project"]
            "#,
        )
        .ok();

        let config = Config::load_from_repo_root(&repo_root).expect("compose must succeed");

        // project's hooks + worktree
        assert_eq!(config.worktree.dir, "/local/{branch}");
        assert_eq!(config.hooks.create.run, vec!["echo project"]);
        // user's integrations override defaults
        assert!(!config.integrations.zoxide.enabled);
        assert!(!config.integrations.fzf.enabled);

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
        std::fs::remove_dir_all(&xdg).ok();
        std::fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    #[serial_test::serial]
    fn test_compose_project_only_uses_default_integrations() {
        // Point XDG at an empty dir so the global file is absent.
        let xdg = std::env::temp_dir().join("ofsht_test_compose_project_only_xdg");
        std::fs::create_dir_all(&xdg).ok();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        let repo_root = std::env::temp_dir().join("ofsht_test_compose_project_only_repo");
        std::fs::create_dir_all(&repo_root).ok();
        std::fs::write(
            repo_root.join(".ofsht.toml"),
            r#"
                [worktree]
                dir = "/project-only/{branch}"

                [hooks.delete]
                run = ["echo bye"]
            "#,
        )
        .ok();

        let config = Config::load_from_repo_root(&repo_root).expect("compose must succeed");

        // project's hooks + worktree
        assert_eq!(config.worktree.dir, "/project-only/{branch}");
        assert_eq!(config.hooks.delete.run, vec!["echo bye"]);
        // integrations fall back to default (zoxide/fzf/gh enabled, tmux Auto)
        assert!(config.integrations.zoxide.enabled);
        assert!(config.integrations.fzf.enabled);
        assert!(config.integrations.gh.enabled);
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Auto);

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
        std::fs::remove_dir_all(&xdg).ok();
        std::fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    #[serial_test::serial]
    fn test_compose_user_only_uses_full_user_config() {
        let xdg = std::env::temp_dir().join("ofsht_test_compose_user_only_xdg");
        let global_dir = xdg.join("ofsht");
        std::fs::create_dir_all(&global_dir).ok();
        std::fs::write(
            global_dir.join("config.toml"),
            r#"
                [worktree]
                dir = "/user-only/{branch}"

                [hooks.create]
                run = ["echo user"]

                [integration.tmux]
                behavior = "always"
                create = "pane"
            "#,
        )
        .ok();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        // Empty repo root — no .ofsht.toml.
        let repo_root = std::env::temp_dir().join("ofsht_test_compose_user_only_repo");
        std::fs::create_dir_all(&repo_root).ok();

        let config = Config::load_from_repo_root(&repo_root).expect("compose must succeed");

        // All fields come from user config
        assert_eq!(config.worktree.dir, "/user-only/{branch}");
        assert_eq!(config.hooks.create.run, vec!["echo user"]);
        assert_eq!(config.integrations.tmux.behavior, TmuxBehavior::Always);
        assert_eq!(config.integrations.tmux.create, OpenMode::Pane);

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
        std::fs::remove_dir_all(&xdg).ok();
        std::fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    #[serial_test::serial]
    fn test_global_config_parse_error_propagates() {
        // Plant a malformed global config and verify the error surfaces
        // instead of being swallowed by the previous `.ok()` fallback.
        let xdg = std::env::temp_dir().join("ofsht_test_global_parse_error_xdg");
        let global_dir = xdg.join("ofsht");
        std::fs::create_dir_all(&global_dir).ok();
        let global_path = global_dir.join("config.toml");
        std::fs::write(&global_path, "this is not = valid toml [[").ok();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        // Use a non-existent local path so the loader falls through to the
        // global file, which is malformed.
        let repo_root = std::env::temp_dir().join("ofsht_test_global_parse_error_repo");
        std::fs::create_dir_all(&repo_root).ok();

        let result = Config::load_from_repo_root(&repo_root);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("Failed to parse config file"),
            "expected parse-error context, got: {msg}"
        );
        assert!(
            msg.contains(global_path.to_string_lossy().as_ref()),
            "expected global path in error, got: {msg}"
        );

        // Clean up
        std::env::remove_var("XDG_CONFIG_HOME");
        std::fs::remove_dir_all(&xdg).ok();
        std::fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn test_local_config_integration_zoxide_only_fails() {
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

        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("unknown field `integration`"),
            "expected unknown-field error, got: {msg}"
        );

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_local_config_integration_gh_only_fails() {
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

        let result = Config::load_from_repo_root(&temp_dir);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("unknown field `integration`"),
            "expected unknown-field error, got: {msg}"
        );

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_template_global_is_valid_toml() {
        let ctx = template_generator::TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };
        let template = ctx.generate_global();
        let result: Result<UserConfig, _> = toml::from_str(&template);
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_local_is_valid_toml() {
        let ctx = template_generator::TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };
        let template = ctx.generate_local();
        let result: Result<ProjectConfig, _> = toml::from_str(&template);
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_global_contains_all_sections() {
        let ctx = template_generator::TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };
        let template = ctx.generate_global();
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
        let ctx = template_generator::TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };
        let template = ctx.generate_local();
        // Local config should NOT contain [worktree] section
        assert!(!template.contains("[worktree]"));
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
        let ctx = template_generator::TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };
        let template = ctx.generate_global();
        assert!(template.contains("# Variables:"));
    }
}
