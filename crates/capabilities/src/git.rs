//! Git repository, branch, detached commit, worktree, submodule, and stash detection.

use std::path::{Path, PathBuf};

use crate::scan::DirSnapshot;
use crate::{CapabilityItem, CapabilityKind, scan};

pub fn detect(ancestors: &[DirSnapshot]) -> Option<CapabilityItem> {
    for dir in ancestors {
        let dot_git = dir.path().join(".git");
        if dir.has_dir(".git") {
            return inspect_git_dir(dir, &dot_git, None);
        }
        if dir.has_file(".git")
            && let Some(line) = dir.read_first_line(".git")
            && let Some(raw_target) = line.strip_prefix("gitdir:")
        {
            let target = PathBuf::from(raw_target.trim());
            let git_dir = if target.is_absolute() {
                target
            } else {
                dir.path().join(target)
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
    repo_root: &DirSnapshot,
    git_dir: &Path,
    link_kind: Option<&str>,
) -> Option<CapabilityItem> {
    let head = scan::read_first_line_path(&git_dir.join("HEAD"))?;
    let (head_short, head_detail) = parse_head(&head);
    let state = detect_operation(git_dir);
    let common_dir = resolve_common_dir(git_dir);
    let stashes = count_stashes(&common_dir);
    let has_submodules = repo_root.has_file(".gitmodules");

    let mut modifiers = Vec::new();
    if let Some(op) = state {
        modifiers.push(op.to_owned());
    } else if let Some(kind) = link_kind {
        modifiers.push(kind.to_owned());
    }
    if stashes > 0 {
        modifiers.push(stash_label(stashes));
    }

    let value = if modifiers.is_empty() {
        head_short
    } else {
        format!("{head_short} ({})", modifiers.join(", "))
    };

    let mut detail = format!("Git: {head_detail}");
    if let Some(op) = state {
        detail = format!("{detail} · {op}");
    }
    if let Some(kind) = link_kind {
        detail = format!("{detail} · {kind}");
    }
    if stashes > 0 {
        detail = format!("{detail} · {}", stash_label(stashes));
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

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Some(rel) = scan::read_first_line_path(&git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let target = PathBuf::from(rel);
    if target.is_absolute() {
        target
    } else {
        git_dir.join(target)
    }
}

fn count_stashes(common_dir: &Path) -> usize {
    if let Some(log) = scan::read_bounded_path(&common_dir.join("logs/refs/stash")) {
        let count = log.lines().filter(|line| !line.trim().is_empty()).count();
        if count > 0 {
            return count;
        }
    }
    if scan::has_file_on_disk(common_dir, "refs/stash") {
        return 1;
    }
    if let Some(packed) = scan::read_bounded_path(&common_dir.join("packed-refs"))
        && packed
            .lines()
            .any(|line| !line.starts_with('#') && line.trim_end().ends_with(" refs/stash"))
    {
        return 1;
    }
    0
}

fn stash_label(count: usize) -> String {
    if count == 1 {
        "1 stash".to_owned()
    } else {
        format!("{count} stashes")
    }
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
    if scan::has_dir_on_disk(git_dir, "rebase-merge")
        || scan::has_dir_on_disk(git_dir, "rebase-apply")
    {
        Some("rebase")
    } else if scan::has_file_on_disk(git_dir, "MERGE_HEAD") {
        Some("merge")
    } else if scan::has_file_on_disk(git_dir, "CHERRY_PICK_HEAD") {
        Some("cherry-pick")
    } else if scan::has_file_on_disk(git_dir, "REVERT_HEAD") {
        Some("revert")
    } else if scan::has_file_on_disk(git_dir, "BISECT_LOG") {
        Some("bisect")
    } else {
        None
    }
}
