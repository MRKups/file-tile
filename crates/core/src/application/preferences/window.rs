//! App-wide window preferences and the final normal-frame close handoff.

use crate::domain::WindowPlacement;

use crate::application::contracts::storage::{CloseStage, PersistenceState, SaveTag};
use crate::application::{Application, Effect};

impl Application {
    pub fn window_placement(&self) -> Option<WindowPlacement> {
        self.configuration
            .as_ref()
            .and_then(|(preferences, _)| preferences.window)
    }

    pub(in crate::application) fn observe_window(
        &mut self,
        placement: WindowPlacement,
    ) -> Vec<Effect> {
        if self.management.is_some() {
            return Vec::new();
        }
        if !matches!(
            self.persistence,
            PersistenceState::Ready {
                closing: CloseStage::Open,
                ..
            }
        ) {
            return Vec::new();
        }
        if self.replace_window(placement) {
            self.save_effect(true)
        } else {
            Vec::new()
        }
    }

    fn replace_window(&mut self, placement: WindowPlacement) -> bool {
        if !placement.valid() {
            return false;
        }
        let Some((preferences, _)) = &mut self.configuration else {
            return false;
        };
        if preferences.window == Some(placement) {
            return false;
        }
        preferences.window = Some(placement);
        true
    }

    pub(in crate::application) fn window_captured_for_close(
        &mut self,
        tag: SaveTag,
        placement: Option<WindowPlacement>,
    ) -> Vec<Effect> {
        let PersistenceState::Ready {
            session,
            id,
            revision,
            attempt,
            closing: CloseStage::SamplingWindow,
            ..
        } = &self.persistence
        else {
            return Vec::new();
        };
        if tag.session != *session
            || tag.workspace != *id
            || tag.revision != *revision
            || tag.attempt != *attempt
        {
            return Vec::new();
        }
        let changed = placement.is_some_and(|placement| self.replace_window(placement));
        if let PersistenceState::Ready { closing, .. } = &mut self.persistence {
            *closing = CloseStage::Flushing;
        }
        crate::diagnostics::session().record(
            crate::diagnostics::Subsystem::Session,
            crate::diagnostics::Activity::Flush,
            crate::diagnostics::Outcome::WaitingSave,
            tag.attempt,
            tag.revision,
            None,
        );
        self.save_effect(changed)
    }
}
