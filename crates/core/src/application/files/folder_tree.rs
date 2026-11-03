//! Configure-owned, bounded immediate-child tree reads, independent of navigation.
use crate::application::contracts::directory::{EntryKind, ListingCompletion, ListingRequest};
use crate::application::{Application, Effect};
use crate::domain::ViewId;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum TreeLoad {
    Unread,
    Queued,
    Loading(ListingRequest),
    Ready {
        children: Vec<PathBuf>,
        partial: bool,
        limited: bool,
        issues: Vec<crate::application::contracts::directory::DirectoryIssue>,
    },
    Failed(String),
}
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub expanded: bool,
    pub load: TreeLoad,
    pub target: PathBuf,
}
#[derive(Debug)]
pub struct FolderTree {
    pub view: ViewId,
    pub roots: Vec<PathBuf>,
    pub nodes: HashMap<PathBuf, TreeNode>,
    reveal: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    range: Option<crate::domain::NavigationRange>,
}
impl FolderTree {
    pub const MAX_NODES: usize = 2048;
    pub fn rows(&self) -> Vec<(usize, &Path)> {
        let mut rows = Vec::new();
        let mut visited = HashSet::new();
        let mut stack: Vec<_> = self
            .roots
            .iter()
            .rev()
            .map(|root| (0, root.as_path()))
            .collect();
        while let Some((depth, path)) = stack.pop() {
            if !visited.insert(path) {
                continue;
            }
            if rows.len() >= Self::MAX_NODES {
                break;
            }
            rows.push((depth, path));
            if let Some(TreeNode {
                expanded: true,
                load: TreeLoad::Ready { children, .. },
                ..
            }) = self.nodes.get(path)
            {
                stack.extend(
                    children
                        .iter()
                        .rev()
                        .map(|path| (depth + 1, path.as_path())),
                );
            }
        }
        rows
    }
    pub fn requests(&self) -> impl Iterator<Item = &ListingRequest> {
        self.nodes.values().filter_map(|node| match &node.load {
            TreeLoad::Loading(request) => Some(request),
            _ => None,
        })
    }
    fn reveal(&mut self, path: &Path) {
        if self.selected.as_deref() == Some(path) {
            return;
        }
        self.selected = Some(path.to_path_buf());
        self.reveal_path(path);
    }
    fn reveal_path(&mut self, path: &Path) {
        let root = self
            .roots
            .iter()
            .find(|root| path.starts_with(root))
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"));
        self.reveal.extend(
            path.ancestors()
                .filter(|path| path.starts_with(&root))
                .map(Path::to_path_buf),
        );
        for (path, node) in &mut self.nodes {
            if self.reveal.contains(path) && !node.expanded {
                node.expanded = true;
                node.load = TreeLoad::Queued;
            }
        }
    }
}
impl Application {
    pub fn folder_tree(&self) -> Option<&FolderTree> {
        self.folder_tree.as_ref()
    }
    pub(in crate::application) fn sync_folder_tree(&mut self) -> Vec<Effect> {
        let context = self
            .workspace
            .boxes()
            .iter()
            .find(|item| self.workspace.configuring(item.id))
            .filter(|_| self.accepts_workspace_input())
            .map(|item| {
                (
                    item.active_view().view,
                    item.active_view().navigation.current().to_path_buf(),
                )
            });
        let Some((view, path)) = context else {
            self.folder_tree = None;
            return vec![];
        };
        if !self
            .folder_tree
            .as_ref()
            .is_some_and(|tree| tree.view == view)
        {
            let roots = vec![
                self.home.clone(),
                PathBuf::from("/Volumes"),
                PathBuf::from("/"),
            ];
            let nodes = roots
                .iter()
                .map(|path| {
                    (
                        path.clone(),
                        TreeNode {
                            expanded: false,
                            load: TreeLoad::Unread,
                            target: path.clone(),
                        },
                    )
                })
                .collect();
            self.folder_tree = Some(FolderTree {
                view,
                roots,
                nodes,
                reveal: HashSet::new(),
                selected: None,
                range: None,
            });
        }
        if let Some(tree) = &mut self.folder_tree {
            tree.reveal(&path);
            let range = self
                .workspace
                .view(view)
                .and_then(|item| item.navigation.scope().range());
            if tree.range.as_ref() != range {
                tree.range = range.cloned();
                if let Some(range) = range {
                    tree.reveal_path(range.start());
                    for stop in range.stops() {
                        tree.reveal_path(stop);
                    }
                }
            }
        }
        self.fill_tree_reads()
    }
    pub(in crate::application) fn toggle_tree(
        &mut self,
        view: ViewId,
        path: PathBuf,
    ) -> Vec<Effect> {
        let Some(tree) = &mut self.folder_tree else {
            return vec![];
        };
        if tree.view != view {
            return vec![];
        }
        let Some(node) = tree.nodes.get_mut(&path) else {
            return vec![];
        };
        if node.expanded {
            node.expanded = false;
            let children = match &node.load {
                TreeLoad::Ready { children, .. } => children.clone(),
                _ => vec![],
            };
            node.load = TreeLoad::Unread;
            let mut stack = children;
            while let Some(child) = stack.pop() {
                if tree.roots.contains(&child) {
                    continue;
                }
                if let Some(TreeNode {
                    load: TreeLoad::Ready { children, .. },
                    ..
                }) = tree.nodes.remove(&child)
                {
                    stack.extend(children);
                }
            }
            tree.reveal
                .retain(|candidate| !candidate.starts_with(&path));
        } else {
            node.expanded = true;
            node.load = TreeLoad::Queued;
        }
        self.fill_tree_reads()
    }
    fn fill_tree_reads(&mut self) -> Vec<Effect> {
        let Some(tree) = &mut self.folder_tree else {
            return vec![];
        };
        let slots = 2usize.saturating_sub(tree.requests().count());
        let paths: Vec<_> = tree
            .rows()
            .into_iter()
            .filter(|(_, path)| {
                matches!(
                    tree.nodes.get(*path).map(|node| &node.load),
                    Some(TreeLoad::Queued)
                )
            })
            .take(slots)
            .map(|(_, path)| path.to_path_buf())
            .collect();
        let mut effects = vec![];
        for directory in paths {
            let Some(generation) = self.next_generation.checked_add(1) else {
                break;
            };
            self.next_generation = generation;
            let request = ListingRequest {
                view: tree.view,
                generation,
                directory: directory.clone(),
            };
            if let Some(node) = tree.nodes.get_mut(&directory) {
                node.load = TreeLoad::Loading(request.clone());
            }
            effects.push(Effect::ListTree(request));
        }
        effects
    }
    pub(in crate::application) fn tree_completed(
        &mut self,
        completion: ListingCompletion,
    ) -> Vec<Effect> {
        let Some(tree) = &mut self.folder_tree else {
            return vec![];
        };
        let path = completion.request.directory;
        if tree.view != completion.request.view
            || !matches!(tree.nodes.get(&path).map(|node| &node.load), Some(TreeLoad::Loading(request)) if request.generation == completion.request.generation)
        {
            return vec![];
        }
        let load = match completion.result {
            Err(error) => TreeLoad::Failed(error.describe("expand folder")),
            Ok(listing) => {
                let entries = listing.entries();
                // Listings sort directories first, including hidden directories
                // and links. Locate that prefix without scanning the file tail.
                let folders =
                    &entries[..entries.partition_point(|entry| entry.kind == EntryKind::Directory)];
                let available = FolderTree::MAX_NODES.saturating_sub(tree.nodes.len());
                let mut children = vec![];
                for entry in folders.iter().take(available) {
                    // Visible branch identity stays separate from its resolved target.
                    let tree_path = path.join(&entry.name);
                    // A directory link back to an ancestor is shown, but not
                    // recursively expanded by automatic path revelation.
                    let expanded = tree.reveal.contains(&tree_path) && !entry.symlink;
                    tree.nodes.entry(tree_path.clone()).or_insert(TreeNode {
                        expanded,
                        load: if expanded {
                            TreeLoad::Queued
                        } else {
                            TreeLoad::Unread
                        },
                        target: listing
                            .entry_target(entry)
                            .unwrap_or(&entry.path)
                            .to_path_buf(),
                    });
                    children.push(tree_path);
                }
                TreeLoad::Ready {
                    children,
                    partial: !listing.issues.is_empty(),
                    limited: folders.len() > available,
                    issues: listing.issues,
                }
            }
        };
        if let Some(node) = tree.nodes.get_mut(&path) {
            node.load = load;
        }
        self.fill_tree_reads()
    }
}
