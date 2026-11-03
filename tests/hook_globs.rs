#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    metadata: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

impl Fixture {
    fn new(separate: bool) -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app with spaces");
        std::fs::create_dir_all(&repo).unwrap();
        let metadata = if separate {
            temp.path().join("metadata ")
        } else {
            repo.join(".git")
        };
        if separate {
            git(
                &repo,
                &[
                    "init",
                    "-qb",
                    "main",
                    "--separate-git-dir",
                    metadata.to_str().unwrap(),
                ],
            );
        } else {
            git(&repo, &["init", "-qb", "main"]);
        }
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        write(
            &temp.path().join("config/ofsht/config.toml"),
            "[integration.zoxide]\nenabled=false\n",
        );
        write(
            &metadata.join("info/exclude"),
            "*.env\n.pool\n.ofsht.toml\n",
        );
        Self {
            temp,
            repo,
            metadata,
        }
    }

    fn worktree(&self, path: &Path, branch: &str) {
        let output = Command::new("git")
            .args(["worktree", "add", "-qb", branch])
            .arg(path)
            .current_dir(&self.repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn configure(&self, action: &str, patterns: &[&str]) {
        write(
            &self.repo.join(".ofsht.toml"),
            &format!(
                "[worktree]\ndir='.pool/{{branch}}'\n[hooks.create]\n{action}={}\n",
                serde_json::to_string(patterns).unwrap()
            ),
        );
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::cargo_bin("ofsht")
            .unwrap()
            .args(args)
            .current_dir(&self.repo)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("TMUX")
            .output()
            .unwrap()
    }

    fn add(&self) -> (PathBuf, Output) {
        let out = self.run(&["add", "destination"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        (self.repo.join(".pool/destination"), out)
    }
}

#[test]
fn glob_files_skip_registered_worktrees_but_include_ignored_main_files() {
    for action in ["copy", "link"] {
        let f = Fixture::new(false);
        for (path, name) in [
            (".pool/sibling", "sibling"),
            ("custom/manual", "manual"),
            ("custom/line\nbreak", "newline"),
        ] {
            let path = f.repo.join(path);
            f.worktree(&path, name);
            write(&path.join("private.env"), "private");
        }
        write(&f.repo.join("main.env"), "main");
        f.configure(action, &["**/*.env"]);
        let (dest, _) = f.add();
        assert_eq!(
            std::fs::read_to_string(dest.join("main.env")).unwrap(),
            "main"
        );
        assert!(!dest.join(".pool").exists());
        assert!(!dest.join("custom").exists());
        write(&dest.join("own.env"), "own");
        let sync = f.run(&["sync", if action == "copy" { "--copy" } else { "--link" }]);
        assert!(
            sync.status.success(),
            "{}",
            String::from_utf8_lossy(&sync.stderr)
        );
        assert!(!dest.join(".pool").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("own.env")).unwrap(),
            "own"
        );
    }
}

#[test]
fn glob_does_not_copy_or_link_git_metadata() {
    for action in ["copy", "link"] {
        let f = Fixture::new(false);
        write(&f.repo.join("data/HEAD"), "ordinary");
        f.configure(action, &["**/HEAD"]);
        let (dest, _) = f.add();
        assert_eq!(
            std::fs::read_to_string(dest.join("data/HEAD")).unwrap(),
            "ordinary"
        );
        assert!(dest.join(".git").is_file());
        assert!(!dest.join(".git/HEAD").exists());
    }
}

#[test]
fn glob_directory_copy_filters_descendants() {
    let f = Fixture::new(false);
    let sibling = f.repo.join("shared/worktree");
    f.worktree(&sibling, "sibling");
    write(&sibling.join("private.env"), "private");
    write(&f.repo.join("shared/.git/secret"), "metadata");
    write(&f.repo.join("shared/main.env"), "main");
    f.configure("copy", &["shared*"]);
    let (dest, _) = f.add();
    assert_eq!(
        std::fs::read_to_string(dest.join("shared/main.env")).unwrap(),
        "main"
    );
    assert!(!dest.join("shared/worktree").exists());
    assert!(!dest.join("shared/.git").exists());
}

#[test]
fn glob_directory_link_skips_excluded_descendants_and_keeps_safe_links() {
    let f = Fixture::new(false);
    f.worktree(&f.repo.join("shared-unsafe/worktree"), "sibling");
    write(&f.repo.join("shared-safe/main.env"), "main");
    f.configure("link", &["shared-*"]);
    let (dest, out) = f.add();
    assert!(!dest.join("shared-unsafe").exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Skipping glob directory link"));
    assert_eq!(
        std::fs::read_to_string(dest.join("shared-safe/main.env")).unwrap(),
        "main"
    );
    assert!(dest
        .join("shared-safe")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn glob_aliases_cannot_reach_worktrees_or_separate_metadata() {
    use std::os::unix::fs::symlink;
    for action in ["copy", "link"] {
        let f = Fixture::new(true);
        let sibling = f.repo.join(".pool/sibling");
        f.worktree(&sibling, "sibling");
        write(&sibling.join("private.env"), "private");
        symlink(&sibling, f.repo.join("alias-worktree")).unwrap();
        symlink(sibling.join("private.env"), f.repo.join("alias-file.env")).unwrap();
        symlink(&f.metadata, f.repo.join("alias-metadata")).unwrap();
        symlink(f.metadata.join("HEAD"), f.repo.join("alias-HEAD")).unwrap();
        f.configure(action, &["alias-*"]);
        let (dest, _) = f.add();
        for name in [
            "alias-worktree",
            "alias-file.env",
            "alias-metadata",
            "alias-HEAD",
        ] {
            assert!(
                dest.join(name).symlink_metadata().is_err(),
                "{action}: {name}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn glob_directory_links_detect_indirect_aliases_and_terminate_cycles() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(false);
    let sibling = f.repo.join(".pool/sibling");
    f.worktree(&sibling, "sibling");
    let shared = f.temp.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    symlink(&sibling, shared.join("private")).unwrap();
    for name in ["bundle-indirect", "bundle-ancestor", "bundle-cycle"] {
        std::fs::create_dir_all(f.repo.join(name)).unwrap();
    }
    symlink(&shared, f.repo.join("bundle-indirect/cache")).unwrap();
    symlink(f.repo.join(".pool"), f.repo.join("bundle-ancestor/cache")).unwrap();
    symlink(".", f.repo.join("bundle-cycle/loop")).unwrap();
    write(&f.repo.join("bundle-cycle/main.env"), "main");
    f.configure("link", &["bundle-*"]);
    let (dest, _) = f.add();
    assert!(!dest.join("bundle-indirect").exists());
    assert!(!dest.join("bundle-ancestor").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("bundle-cycle/main.env")).unwrap(),
        "main"
    );
}

#[cfg(unix)]
#[test]
fn glob_directory_copy_does_not_follow_directory_symlinks() {
    let f = Fixture::new(false);
    write(&f.repo.join("bundle/main.env"), "main");
    let outside = f.temp.path().join("outside");
    write(&outside.join("external.env"), "external");
    std::os::unix::fs::symlink(&outside, f.repo.join("bundle/alias")).unwrap();
    f.configure("copy", &["bundle*"]);
    let (dest, _) = f.add();
    assert!(!dest.join("bundle/alias").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("bundle/main.env")).unwrap(),
        "main"
    );
}

#[test]
fn literal_directories_keep_explicit_copy_and_link_semantics() {
    for action in ["copy", "link"] {
        let f = Fixture::new(false);
        write(&f.repo.join("explicit/.git/marker"), "explicit");
        f.configure(action, &["explicit"]);
        let (dest, _) = f.add();
        assert_eq!(
            std::fs::read_to_string(dest.join("explicit/.git/marker")).unwrap(),
            "explicit"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn glob_exclusions_preserve_non_utf8_worktree_and_metadata_paths() {
    use std::ffi::OsString;
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    for action in ["copy", "link"] {
        let f = Fixture::new(false);
        let metadata = f
            .temp
            .path()
            .join(OsString::from_vec(b"metadata\xff".to_vec()));
        let output = Command::new("git")
            .args(["init", "-q", "--separate-git-dir"])
            .arg(&metadata)
            .current_dir(&f.repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let sibling = f
            .repo
            .join("custom")
            .join(OsString::from_vec(b"worktree\xff".to_vec()));
        f.worktree(&sibling, "sibling");
        write(&sibling.join("private.env"), "private");
        write(&f.repo.join("main.env"), "main");
        symlink(metadata.join("HEAD"), f.repo.join("alias-HEAD")).unwrap();
        f.configure(action, &["**/*.env", "alias-*"]);
        let (dest, _) = f.add();
        assert!(!dest.join("custom").exists());
        assert!(dest.join("alias-HEAD").symlink_metadata().is_err());
        assert_eq!(
            std::fs::read_to_string(dest.join("main.env")).unwrap(),
            "main"
        );
    }
}
