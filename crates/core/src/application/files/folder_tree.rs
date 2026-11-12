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
    auto_expand: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRootMeta {
    pub label: String,
    pub kind: crate::platform::volumes::LocationKind,
}
#[derive(Debug)]
pub struct FolderTree {
    pub view: ViewId,
    pub roots: Vec<PathBuf>,
    pub root_meta: HashMap<PathBuf, TreeRootMeta>,
    pub nodes: HashMap<PathBuf, TreeNode>,
    reveal: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    range: Option<crate::domain::NavigationRange>,
}
impl FolderTree {
    pub const MAX_NODES: usize = 2048;
    pub fn root_meta(&self, path: &Path) -> Option<&TreeRootMeta> {
        self.root_meta.get(path)
    }
    pub fn rows(&self) -> Vec<(usize, &Path)> {
        let mut rows = Vec::new();
        let mut visited = HashSet::new();
        let root_set: HashSet<&Path> = self.roots.iter().map(PathBuf::as_path).collect();
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
                        .filter(|child| {
                            !root_set.contains(child.as_path())
                                && self.nodes.contains_key(child.as_path())
                        })
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
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.as_os_str().len())
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"));
        self.reveal.extend(
            path.ancestors()
                .filter(|path| path.starts_with(&root))
                .map(Path::to_path_buf),
        );
        for (path, node) in &mut self.nodes {
            if self.reveal.contains(path) && !node.expanded && node.auto_expand {
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
    fn build_tree_roots(&self) -> (Vec<PathBuf>, HashMap<PathBuf, TreeRootMeta>) {
        let mut roots = vec![self.home.clone()];
        let mut seen = HashSet::from([self.home.clone()]);
        let mut root_meta = HashMap::new();
        for loc in &self.locations {
            if crate::application::contracts::configuration::validate_saved_path(&loc.path).is_ok()
                && seen.insert(loc.path.clone())
            {
                roots.push(loc.path.clone());
                root_meta.insert(
                    loc.path.clone(),
                    TreeRootMeta {
                        label: loc.label.clone(),
                        kind: loc.kind.clone(),
                    },
                );
            }
        }
        for fixed in [PathBuf::from("/Volumes"), PathBuf::from("/")] {
            if seen.insert(fixed.clone()) {
                roots.push(fixed);
            }
        }
        (roots, root_meta)
    }
    pub(in crate::application) fn reconcile_tree_roots(&mut self) -> Vec<Effect> {
        if self.folder_tree.is_none() {
            return vec![];
        }
        let (roots, root_meta) = self.build_tree_roots();
        if let Some(tree) = &mut self.folder_tree {
            let new_set: HashSet<&Path> = roots.iter().map(PathBuf::as_path).collect();
            let removed: Vec<PathBuf> = tree
                .roots
                .iter()
                .filter(|old| !new_set.contains(old.as_path()))
                .cloned()
                .collect();
            for stale_root in &removed {
                tree.nodes.retain(|node_path, _| {
                    !node_path.starts_with(stale_root) || new_set.contains(node_path.as_path())
                });
                for node in tree.nodes.values_mut() {
                    if let TreeLoad::Ready { children, .. } = &mut node.load {
                        children.retain(|child| {
                            !child.starts_with(stale_root) || new_set.contains(child.as_path())
                        });
                    }
                }
                tree.reveal.retain(|candidate| {
                    !candidate.starts_with(stale_root) || new_set.contains(candidate.as_path())
                });
            }
            for root in &roots {
                tree.nodes.entry(root.clone()).or_insert_with(|| TreeNode {
                    expanded: false,
                    load: TreeLoad::Unread,
                    target: root.clone(),
                    auto_expand: true,
                });
            }
            tree.roots = roots;
            tree.root_meta = root_meta;
            if let Some(selected) = tree.selected.clone() {
                tree.reveal_path(&selected);
            }
            if let Some(range) = tree.range.clone() {
                tree.reveal_path(range.start());
                for stop in range.stops() {
                    tree.reveal_path(stop);
                }
            }
        }
        self.fill_tree_reads()
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
            let (roots, root_meta) = self.build_tree_roots();
            let nodes = roots
                .iter()
                .map(|path| {
                    (
                        path.clone(),
                        TreeNode {
                            expanded: false,
                            load: TreeLoad::Unread,
                            target: path.clone(),
                            auto_expand: true,
                        },
                    )
                })
                .collect();
            self.folder_tree = Some(FolderTree {
                view,
                roots,
                root_meta,
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
        let Some(view) = self.folder_tree.as_ref().map(|tree| tree.view) else {
            return vec![];
        };
        let policy = self.safety_policy(Some(view));
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
            let blocked = policy.check_read_directory(&directory).err().or_else(|| {
                tree.nodes
                    .get(&directory)
                    .and_then(|node| policy.check_read_directory(&node.target).err())
            });
            if let Some(reason) = blocked {
                if let Some(node) = tree.nodes.get_mut(&directory) {
                    node.load = TreeLoad::Failed(reason.to_owned());
                }
                continue;
            }
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
        let Some(view) = self.folder_tree.as_ref().map(|tree| tree.view) else {
            return vec![];
        };
        let policy = self.safety_policy(Some(view));
        let mount_err = completion
            .result
            .as_ref()
            .ok()
            .and_then(|listing| self.check_listing_mount_access(listing).err());
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
                if let Some(reason) = policy
                    .check_read_directory(&listing.directory)
                    .err()
                    .or(mount_err)
                {
                    TreeLoad::Failed(reason.to_owned())
                } else {
                    let entries = listing.entries();
                    // Listings sort directories first, including hidden directories
                    // and links. Locate that prefix without scanning the file tail.
                    let folders = &entries
                        [..entries.partition_point(|entry| entry.kind == EntryKind::Directory)];
                    let available = FolderTree::MAX_NODES.saturating_sub(tree.nodes.len());
                    let mut children = vec![];
                    for entry in folders.iter().take(available) {
                        // Visible branch identity stays separate from its resolved target.
                        let tree_path = path.join(&entry.name);
                        // A directory link back to an ancestor or a dataless placeholder
                        // is shown, but not recursively expanded by automatic revelation.
                        let auto_expand = !entry.symlink && !entry.dataless;
                        let expanded = tree.reveal.contains(&tree_path) && auto_expand;
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
                            auto_expand,
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
            }
        };
        if let Some(node) = tree.nodes.get_mut(&path) {
            node.load = load;
        }
        self.fill_tree_reads()
    }
}
