//! Bounded dirty tags and deadlines. Continuous changes cannot postpone delivery.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::application::contracts::watching::ObservationTag;

pub(super) const QUIET: Duration = Duration::from_millis(250);
pub(super) const MAX_DELAY: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct Pending {
    tags: HashSet<ObservationTag>,
    first: Option<Instant>,
    last: Option<Instant>,
    pub recheck: bool,
}

impl Pending {
    pub fn add(
        &mut self,
        tags: impl IntoIterator<Item = ObservationTag>,
        now: Instant,
        recheck: bool,
    ) {
        let mut any = false;
        for tag in tags {
            self.tags.insert(tag);
            any = true;
        }
        if any {
            self.first.get_or_insert(now);
            self.last = Some(now);
            self.recheck |= recheck;
        }
    }

    pub fn retain(&mut self, current: &HashSet<ObservationTag>) {
        self.tags.retain(|tag| current.contains(tag));
        if self.tags.is_empty() {
            *self = Self::default();
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        Some((self.first? + MAX_DELAY).min(self.last? + QUIET))
    }

    pub fn take(&mut self) -> Vec<ObservationTag> {
        let tags = self.tags.drain().collect();
        self.first = None;
        self.last = None;
        self.recheck = false;
        tags
    }
}
