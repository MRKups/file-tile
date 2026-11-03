//! Startup-only backup discovery and explicit, original-preserving restoration.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::application::contracts::configuration::{ConfigurationDocument, WorkspaceId};
use crate::application::contracts::storage::{BackupOffer, StorageFailure};

use super::files::{Temporary, read_optional, sync_directory};
use super::{ConfigError, ConfigKey, decode};

#[derive(Clone)]
struct Source {
    key: ConfigKey,
    original: Option<Arc<[u8]>>,
    bytes: Arc<[u8]>,
    document: Arc<ConfigurationDocument>,
    backup: bool,
}

pub(super) struct RecoveryPlan {
    pub(super) token: WorkspaceId,
    sources: Vec<Source>,
    #[cfg(test)]
    pub(super) fail_before: Option<ConfigKey>,
}

impl RecoveryPlan {
    pub(super) fn inspect(config: &Path) -> Result<Option<Self>, StorageFailure> {
        let preferences = sources(config, ConfigKey::Preferences)?;
        let index = sources(config, ConfigKey::WorkspaceIndex)?;
        let mut best: Option<Self> = None;
        // At most two sources per kind and two referenced workspace identities.
        // Prefer the coherent candidate requiring the fewest replacements.
        for preferences in &preferences {
            let ConfigurationDocument::Preferences(record) = preferences.document.as_ref() else {
                continue;
            };
            let Some(id) = record.last_workspace_id else {
                continue;
            };
            let workspaces = sources(config, ConfigKey::Workspace(id))?;
            for index in &index {
                let ConfigurationDocument::WorkspaceIndex(record) = index.document.as_ref() else {
                    continue;
                };
                if !record.workspaces.iter().any(|item| item.id == id) {
                    continue;
                }
                for workspace in &workspaces {
                    let sources = vec![preferences.clone(), index.clone(), workspace.clone()];
                    let replacements = sources.iter().filter(|source| source.backup).count();
                    if replacements > 0
                        && best
                            .as_ref()
                            .is_none_or(|best| replacements < best.replacements())
                    {
                        best = Some(Self {
                            token: WorkspaceId::random(),
                            sources,
                            #[cfg(test)]
                            fail_before: None,
                        });
                    }
                }
            }
        }
        Ok(best)
    }

    fn replacements(&self) -> usize {
        self.sources.iter().filter(|source| source.backup).count()
    }

    pub(super) fn offer(&self) -> BackupOffer {
        BackupOffer {
            token: self.token,
            documents: self
                .sources
                .iter()
                .filter(|source| source.backup)
                .map(|source| source.key.filename())
                .collect(),
        }
    }

    pub(super) fn restore(mut self, config: &Path) -> Result<usize, StorageFailure> {
        self.verify(config)?;
        // Preserve every affected original before replacing any primary. A hard
        // link of our synchronized temporary file publishes without clobbering.
        for source in self.sources.iter().filter(|source| source.backup) {
            if let Some(original) = &source.original {
                let temporary = Temporary::write(config, original)?;
                let preserved =
                    config.join(format!("{}.recovery-{}", source.key.filename(), self.token));
                fs::hard_link(&temporary.path, preserved)?;
            }
        }
        sync_directory(config)?;
        self.verify(config)?;
        self.sources.sort_by_key(|source| match source.key {
            ConfigKey::Workspace(_) => 0,
            ConfigKey::WorkspaceIndex => 1,
            ConfigKey::Preferences => 2,
        });
        let replacements = self.replacements();
        for source in self.sources.iter().filter(|source| source.backup) {
            #[cfg(test)]
            if self.fail_before == Some(source.key) {
                return Err(StorageFailure::Io(std::io::ErrorKind::Other));
            }
            let path = config.join(source.key.filename());
            if read_optional(&path, source.key.byte_limit())?.as_deref()
                != source.original.as_deref()
            {
                return Err(StorageFailure::ChangedExternally);
            }
            let temporary = Temporary::write(config, &source.bytes)?;
            fs::rename(&temporary.path, path)?;
            sync_directory(config)?;
        }
        Ok(replacements)
    }

    fn verify(&self, config: &Path) -> Result<(), StorageFailure> {
        if !fs::symlink_metadata(config)?.is_dir() {
            return Err(StorageFailure::UnsafePath);
        }
        for source in &self.sources {
            let current =
                read_optional(&config.join(source.key.filename()), source.key.byte_limit())?;
            if current.as_deref() != source.original.as_deref() {
                return Err(StorageFailure::ChangedExternally);
            }
            if source.backup {
                let backup = read_optional(
                    &config.join(backup_name(source.key)),
                    source.key.byte_limit(),
                )?;
                if backup.as_deref() != Some(source.bytes.as_ref()) {
                    return Err(StorageFailure::ChangedExternally);
                }
            }
        }
        Ok(())
    }
}

fn backup_name(key: ConfigKey) -> String {
    format!("{}.bak", key.filename())
}

fn sources(config: &Path, key: ConfigKey) -> Result<Vec<Source>, StorageFailure> {
    let original: Option<Arc<[u8]>> =
        read_optional(&config.join(key.filename()), key.byte_limit())?.map(Arc::from);
    let mut sources = Vec::new();
    if let Some(bytes) = &original {
        match decode(bytes, key) {
            Ok(decoded) if decoded.can_save() => sources.push(Source {
                key,
                original: original.clone(),
                bytes: Arc::clone(bytes),
                document: Arc::new(decoded.document),
                backup: false,
            }),
            // Never propose downgrading an inspected unsupported document.
            Err(ConfigError::UnsupportedFormat | ConfigError::UnsupportedVersion(_)) => {
                return Err(StorageFailure::ReadOnly);
            }
            _ => {}
        }
    }
    if let Ok(Some(bytes)) = read_optional(&config.join(backup_name(key)), key.byte_limit())
        && let Ok(decoded) = decode(&bytes, key)
        && decoded.can_save()
    {
        sources.push(Source {
            key,
            original,
            bytes: Arc::from(bytes),
            document: Arc::new(decoded.document),
            backup: true,
        });
    }
    Ok(sources)
}
