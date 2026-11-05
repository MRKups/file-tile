//! Captures the Git commit and working-tree modification state (`<short-sha>` or `<short-sha>+modified`)
//! for `file-tile-core` so runtime UI and diagnostic logs share one build identity.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let root = workspace_root();
    emit("cargo:rerun-if-env-changed=FILE_TILE_GIT_COMMIT");
    for git_path in [".git/HEAD", ".git/index", ".git/packed-refs"] {
        let candidate = root.join(git_path);
        if candidate.exists() {
            emit(&format!("cargo:rerun-if-changed={}", candidate.display()));
        }
    }
    let commit = resolve_git_commit(&root);
    emit(&format!("cargo:rustc-env=FILE_TILE_GIT_COMMIT={commit}"));
}

fn workspace_root() -> PathBuf {
    env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|dir| dir.parent()?.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_git_commit(root: &Path) -> String {
    if let Ok(explicit) = env::var("FILE_TILE_GIT_COMMIT") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    let Some(sha) = git_stdout(
        root,
        &["--no-optional-locks", "rev-parse", "--short=7", "HEAD"],
    )
    .filter(|text| !text.is_empty()) else {
        return "unknown".to_owned();
    };
    let modified = git_stdout(
        root,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
        ],
    )
    .is_some_and(|status| !status.is_empty());
    if modified {
        format!("{sha}+modified")
    } else {
        sha
    }
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}

fn emit(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.write_all(b"\n");
}
