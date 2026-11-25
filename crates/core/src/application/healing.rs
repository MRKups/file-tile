//! Discrete runtime self-healing scenarios for folder navigation and views.
//!
//! Each scenario has a specific trigger condition and bounded recovery action
//! so runtime filesystem disappearances are handled predictably without mixing
//! into general navigation logic.

use std::path::PathBuf;

use crate::application::contracts::directory::{DirectoryError, DirectoryErrorKind};
use crate::application::{Application, Effect};
use crate::domain::{NavigationIntent, ViewId};

impl Application {
    /// Evaluates whether a failed directory listing matches a runtime
    /// self-healing scenario and returns the recovery effect if applicable:
    /// step up to the nearest allowed parent folder, or fall back to the
    /// box's configured starting folder.
    pub(in crate::application) fn try_heal_directory_error(
        &mut self,
        view: ViewId,
        intent: NavigationIntent,
        error: &DirectoryError,
    ) -> Option<Effect> {
        if error.kind != DirectoryErrorKind::NotFound {
            return None;
        }
        // Only heal folders that the view was already resting on or restoring,
        // never an explicit user-typed Configure path or forward/back click.
        if !matches!(
            intent,
            NavigationIntent::Reload | NavigationIntent::Restore | NavigationIntent::Activate
        ) {
            return None;
        }

        let (up_candidate, start_candidate, current) = {
            let item = self.workspace.view(view)?;
            let nav = &item.navigation;
            (
                nav.up_target().map(PathBuf::from),
                nav.return_target().to_path_buf(),
                nav.current().to_path_buf(),
            )
        };

        let scope = self.read_scope(view);
        let policy = self.safety_policy(Some(view));

        // Scenario 1: Step up to the parent folder if allowed by scope and safety.
        if let Some(parent) = up_candidate
            && parent != current
            && scope.allows(&parent)
            && policy.check_read_directory(&parent).is_ok()
            && let Some(effect) = self.request(view, parent, NavigationIntent::Open)
        {
            return Some(effect);
        }

        // Scenario 2: Fall back to the box's configured starting folder if distinct and allowed.
        if start_candidate != current
            && scope.allows(&start_candidate)
            && policy.check_read_directory(&start_candidate).is_ok()
            && let Some(effect) = self.request(view, start_candidate, NavigationIntent::Open)
        {
            return Some(effect);
        }

        None
    }
}
