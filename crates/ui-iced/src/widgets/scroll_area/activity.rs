//! Widget-local idle fade. Schedule only the deadline and the short transition.
use iced::advanced::Shell;
use std::time::{Duration, Instant};

const DELAY: Duration = Duration::from_secs(2);
const FADE: Duration = Duration::from_millis(450);
const IDLE: f32 = 0.18;

pub(super) struct Activity {
    last: Option<Instant>,
    pub opacity: f32,
    pub hovered: bool,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            last: None,
            opacity: 1.0,
            hovered: false,
        }
    }
}

impl Activity {
    pub fn wake(&mut self, now: Instant) {
        self.last = Some(now);
        self.opacity = 1.0;
    }

    pub fn update(&mut self, now: Instant, active: bool, shell: &mut Shell<'_, crate::Message>) {
        if self.last.is_none() || active {
            self.wake(now);
        }
        let previous = self.opacity;
        if let Some(last) = self.last {
            let deadline = last + DELAY;
            let elapsed = now.saturating_duration_since(deadline);
            let progress = (elapsed.as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0);
            self.opacity = 1.0 - (1.0 - IDLE) * progress;
            if !active {
                if now < deadline {
                    shell.request_redraw_at(deadline);
                } else if progress < 1.0 {
                    shell.request_redraw();
                }
            }
        }
        if previous != self.opacity {
            shell.request_redraw();
        }
    }
}
