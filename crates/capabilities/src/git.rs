//! Git repository, branch, detached commit, worktree, and submodule detection.

use std::path::{Path, PathBuf};

use crate::{CapabilityItem, CapabilityKind, scan};

pub fn detect(ancestors: &[PathBuf]) -> Option<CapabilityItem> {
    for dir in ancestors {
        let dot_git = dir.join(".git");
        if scan::has_dir(dir, ".git") {
            return inspect_git_dir(dir, &dot_git, None);
        }
        if scan::has_file(dir, ".git")
            && let Some(line) = scan::read_first_line(&dot_git)
            && let Some(raw_target) = line.strip_prefix("gitdir:")
        {
            let target = PathBuf::from(raw_target.trim());
            let git_dir = if target.is_absolute() {
                target
            } else {
                dir.join(target)
            };
            let link_kind = if git_dir.to_string_lossy().contains("/worktrees/") {
                Some("worktree")
            } else {
                Some("submodule")
            };
            return inspect_git_dir(dir, &git_dir, link_kind);
        }
    }
    None
}

fn inspect_git_dir(
    repo_root: &Path,
    git_dir: &Path,
    link_kind: Option<&str>,
) -> Option<CapabilityItem> {
    let head = scan::read_first_line(&git_dir.join("HEAD"))?;
    let (head_short, head_detail) = parse_head(&head);
    let state = detect_operation(git_dir);
    let has_submodules = scan::has_file(repo_root, ".gitmodules");

    let mut value = head_short;
    if let Some(op) = state {
        value = format!("{value} ({op})");
    } else if let Some(kind) = link_kind {
        value = format!("{value} ({kind})");
    }

    let mut detail = format!("Git: {head_detail}");
    if let Some(op) = state {
        detail = format!("{detail} · {op}");
    }
    if let Some(kind) = link_kind {
        detail = format!("{detail} · {kind}");
    }
    if has_submodules {
        detail = format!("{detail} · submodules");
    }

    Some(CapabilityItem::new(
        CapabilityKind::Git,
        "git",
        Some(value),
        detail,
    ))
}

fn parse_head(head: &str) -> (String, String) {
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        if let Some(branch) = reference.strip_prefix("refs/heads/") {
            return (branch.to_owned(), format!("branch {branch}"));
        }
        if let Some(tag) = reference.strip_prefix("refs/tags/") {
            return (tag.to_owned(), format!("tag {tag}"));
        }
        return (reference.to_owned(), format!("ref {reference}"));
    }
    let short: String = head.chars().take(7).collect();
    (
        format!("{short} (detached)"),
        format!("detached HEAD {short}"),
    )
}

fn detect_operation(git_dir: &Path) -> Option<&'static str> {
    if scan::has_dir(git_dir, "rebase-merge") || scan::has_dir(git_dir, "rebase-apply") {
        Some("rebase")
    } else if scan::has_file(git_dir, "MERGE_HEAD") {
        Some("merge")
    } else if scan::has_file(git_dir, "CHERRY_PICK_HEAD") {
        Some("cherry-pick")
    } else if scan::has_file(git_dir, "REVERT_HEAD") {
        Some("revert")
    } else if scan::has_file(git_dir, "BISECT_LOG") {
        Some("bisect")
    } else {
        None
    }
}
