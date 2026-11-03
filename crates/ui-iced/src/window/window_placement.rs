//! Bounded native geometry sampling through Iced, independent of canvas sizing.

use iced::{Point, Size, Task, window};

use crate::application::Action;
use crate::application::contracts::storage::{PersistenceState, SaveTag};
use crate::domain::{DisplayArea, WindowPlacement, restore_window};

use crate::{Desktop, Message};

#[derive(Debug, Clone, Copy)]
pub(crate) enum SampleKind {
    Bootstrap,
    Observed,
    Positioned,
    Restored,
    Close(SaveTag),
}

#[derive(Debug, Clone)]
pub(crate) enum SamplePart {
    Position(Option<Point>),
    Size(Size),
    Maximized(bool),
    Minimized(Option<bool>),
    Mode(window::Mode),
    Monitor(Option<Size>),
    Displays(Result<Vec<DisplayArea>, String>),
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    placement: Option<WindowPlacement>,
    monitor: Option<Size>,
    displays: Vec<DisplayArea>,
    display_error: Option<String>,
}

#[derive(Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Waiting,
    Restoring,
    Observing,
}

struct PendingSample {
    generation: u64,
    handle: iced::task::Handle,
}
impl Drop for PendingSample {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[derive(Default)]
pub(crate) struct WindowRuntime {
    id: Option<window::Id>,
    generation: u64,
    pending: Option<PendingSample>,
    bootstrap: Option<Snapshot>,
    initial: Option<WindowPlacement>,
    stage: Stage,
    adjusted: bool,
    restore_target: Option<WindowPlacement>,
    restore_attempts: u8,
    move_attempts: u8,
}

impl WindowRuntime {
    pub(crate) fn id(&self) -> Option<window::Id> {
        self.id
    }
}

impl Desktop {
    pub(crate) fn window_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowEvent(id, event) => {
                let task = self.window_event(id, &event);
                Task::batch([
                    task,
                    if matches!(event, window::Event::Opened { .. }) {
                        self.install_quit_menu()
                    } else {
                        Task::none()
                    },
                    self.input(iced::Event::Window(event), iced::event::Status::Ignored),
                ])
            }
            Message::WindowSampled {
                generation,
                kind,
                parts,
            } => self.window_sampled(generation, kind, parts),
            _ => Task::none(),
        }
    }
    pub(crate) fn window_event(&mut self, id: window::Id, event: &window::Event) -> Task<Message> {
        match event {
            window::Event::Opened { position, size } => {
                self.window_runtime.id = Some(id);
                self.window_runtime.initial = position.and_then(|position| {
                    WindowPlacement::observed(position.x, position.y, size.width, size.height)
                });
                self.sample_window(SampleKind::Bootstrap)
            }
            window::Event::Closed => {
                self.window_runtime.pending = None;
                self.window_runtime.id = None;
                self.clear_active_pickers();
                Task::none()
            }
            window::Event::Moved(_)
            | window::Event::Resized(_)
            | window::Event::Rescaled(_)
            | window::Event::Focused => {
                self.window_runtime.generation = self.window_runtime.generation.saturating_add(1);
                if self.window_runtime.pending.is_some() {
                    return Task::none();
                }
                if self.window_runtime.stage == Stage::Waiting {
                    self.window_runtime.bootstrap = None;
                    return self.sample_window(SampleKind::Bootstrap);
                }
                if self.window_runtime.stage != Stage::Observing
                    || !self.application.accepts_edits()
                {
                    return Task::none();
                }
                self.sample_window(SampleKind::Observed)
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn sample_window(&mut self, kind: SampleKind) -> Task<Message> {
        let Some(id) = self.window_runtime.id else {
            return match kind {
                SampleKind::Close(tag) => {
                    Task::done(Message::Action(Action::WindowCapturedForClose {
                        tag,
                        placement: None,
                    }))
                }
                _ => Task::none(),
            };
        };
        self.window_runtime.pending = None;
        self.window_runtime.generation = self.window_runtime.generation.saturating_add(1);
        let generation = self.window_runtime.generation;
        let mut queries = vec![
            window::position(id).map(SamplePart::Position),
            window::size(id).map(SamplePart::Size),
            window::is_maximized(id).map(SamplePart::Maximized),
            window::is_minimized(id).map(SamplePart::Minimized),
            window::mode(id).map(SamplePart::Mode),
            window::monitor_size(id).map(SamplePart::Monitor),
        ];
        if matches!(kind, SampleKind::Bootstrap) {
            #[cfg(target_os = "macos")]
            queries.push(window::run(id, |_| query_display_areas()).map(SamplePart::Displays));
        }
        let task = Task::batch(queries)
            .collect()
            .map(move |parts| Message::WindowSampled {
                generation,
                kind,
                parts,
            });
        let (task, handle) = task.abortable();
        self.window_runtime.pending = Some(PendingSample { generation, handle });
        task
    }

    pub(crate) fn window_sampled(
        &mut self,
        generation: u64,
        kind: SampleKind,
        parts: Vec<SamplePart>,
    ) -> Task<Message> {
        if !self
            .window_runtime
            .pending
            .as_ref()
            .is_some_and(|pending| pending.generation == generation)
        {
            return Task::none();
        }
        self.window_runtime.pending = None;
        if generation != self.window_runtime.generation {
            return self.sample_window(kind);
        }
        let snapshot = snapshot(parts);
        match kind {
            SampleKind::Bootstrap => {
                self.window_runtime.bootstrap = Some(snapshot);
                self.restore_window_if_ready()
            }
            SampleKind::Positioned => {
                let positioned = snapshot
                    .placement
                    .zip(self.window_runtime.restore_target)
                    .is_some_and(|(actual, target)| actual.x == target.x && actual.y == target.y);
                if positioned {
                    self.resize_restored_window()
                } else if self.window_runtime.move_attempts < 3 {
                    self.move_restored_window()
                } else {
                    self.window_runtime.stage = Stage::Observing;
                    self.dispatch(Action::WindowRestorationUnavailable(
                        "The OS did not confirm the saved position. Saved bounds were retained."
                            .into(),
                    ))
                }
            }
            SampleKind::Restored => {
                // AppKit may not have applied a resize/move when the immediate
                // query completes. Never save that intermediate frame over the
                // requested bounds; later native events use the observed path.
                if snapshot.placement != self.window_runtime.restore_target {
                    if self.window_runtime.restore_attempts < 3 {
                        return self.resize_restored_window();
                    }
                    self.window_runtime.stage = Stage::Observing;
                    return self.dispatch(Action::WindowRestorationUnavailable(
                        "The OS did not confirm the requested size after moving to the saved display. Native restart verification is needed.".into()));
                }
                self.window_runtime.stage = Stage::Observing;
                snapshot.placement.map_or_else(Task::none, |placement| {
                    self.dispatch(if self.window_runtime.adjusted {
                        Action::WindowPlacementAdjusted(placement)
                    } else {
                        Action::WindowPlacementObserved(placement)
                    })
                })
            }
            SampleKind::Observed => snapshot.placement.map_or_else(Task::none, |placement| {
                self.dispatch(Action::WindowPlacementObserved(placement))
            }),
            SampleKind::Close(tag) => {
                self.window_runtime.stage = Stage::Observing;
                self.dispatch(Action::WindowCapturedForClose {
                    tag,
                    placement: snapshot.placement,
                })
            }
        }
    }

    pub(crate) fn restore_window_if_ready(&mut self) -> Task<Message> {
        if self.window_runtime.stage != Stage::Waiting
            || !matches!(
                self.application.persistence(),
                PersistenceState::Ready { .. }
            )
        {
            return Task::none();
        }
        let Some(snapshot) = self.window_runtime.bootstrap.clone() else {
            return Task::none();
        };
        self.window_runtime.stage = Stage::Observing;
        if let Some(error) = snapshot.display_error {
            return self.dispatch(Action::WindowRestorationUnavailable(error));
        }
        if self.window_runtime.initial != snapshot.placement {
            // Do not override a user/OS move during slow startup. The current
            // monitor may no longer be the initial primary display.
            return snapshot.placement.map_or_else(Task::none, |placement| {
                self.dispatch(Action::WindowPlacementObserved(placement))
            });
        }
        let (Some(_), Some(_)) = (self.window_runtime.id, snapshot.placement) else {
            return Task::none();
        };
        let mut displays = snapshot.displays;
        // Non-macOS builds retain the conservative single-monitor fallback.
        // On macOS a failed inventory must not masquerade as a successful one.
        if !cfg!(target_os = "macos")
            && displays.is_empty()
            && let Some(monitor) = snapshot.monitor
            && let Some(frame) = WindowPlacement::observed(0.0, 0.0, monitor.width, monitor.height)
        {
            displays.push(DisplayArea { frame, work: frame });
        }
        let Some(restored) = restore_window(self.application.window_placement(), &displays) else {
            return self.dispatch(Action::WindowRestorationUnavailable(
                "No usable display bounds were reported.".into(),
            ));
        };
        self.window_runtime.stage = Stage::Restoring;
        self.window_runtime.adjusted = restored.adjusted;
        let placement = restored.placement;
        self.window_runtime.restore_target = Some(placement);
        // Native actions precede the confirmation sample. Canvas geometry still
        // comes exclusively from its redraw-time sensor, not these query results.
        // A resize on the launch display can be constrained by its work area.
        // Move first, then cross an event-loop query round trip before resizing
        // so Iced has observed the destination display's backing scale.
        self.move_restored_window()
    }

    fn move_restored_window(&mut self) -> Task<Message> {
        let (Some(id), Some(placement)) =
            (self.window_runtime.id, self.window_runtime.restore_target)
        else {
            return Task::none();
        };
        self.window_runtime.move_attempts += 1;
        let task = window::move_to(id, Point::new(placement.x as f32, placement.y as f32));
        task.chain(self.sample_window(SampleKind::Positioned))
    }

    fn resize_restored_window(&mut self) -> Task<Message> {
        let (Some(id), Some(placement)) =
            (self.window_runtime.id, self.window_runtime.restore_target)
        else {
            return Task::none();
        };
        self.window_runtime.restore_attempts += 1;
        window::resize(
            id,
            Size::new(placement.width as f32, placement.height as f32),
        )
        .chain(self.sample_window(SampleKind::Restored))
    }
}

fn snapshot(parts: Vec<SamplePart>) -> Snapshot {
    let (mut displays, mut display_error) = (Vec::new(), None);
    let (mut position, mut size, mut maximized, mut minimized, mut mode, mut monitor) =
        (None, None, None, None, None, None);
    for part in parts {
        match part {
            SamplePart::Position(value) => position = value,
            SamplePart::Size(value) => size = Some(value),
            SamplePart::Maximized(value) => maximized = Some(value),
            SamplePart::Minimized(value) => minimized = value,
            SamplePart::Mode(value) => mode = Some(value),
            SamplePart::Monitor(value) => monitor = value,
            SamplePart::Displays(Ok(value)) => displays = value,
            SamplePart::Displays(Err(error)) => display_error = Some(error),
        }
    }
    let placement = if maximized == Some(false)
        && minimized == Some(false)
        && mode == Some(window::Mode::Windowed)
    {
        position.zip(size).and_then(|(position, size)| {
            WindowPlacement::observed(position.x, position.y, size.width, size.height)
        })
    } else {
        None
    };
    Snapshot {
        placement,
        monitor,
        displays,
        display_error,
    }
}

#[cfg(target_os = "macos")]
fn query_display_areas() -> Result<Vec<DisplayArea>, String> {
    let raw = crate::platform::display_areas()?;
    let mut areas = Vec::with_capacity(raw.len());
    for ((fx, fy, fw, fh), (wx, wy, ww, wh)) in raw {
        let Some(frame) = WindowPlacement::observed(fx, fy, fw, fh) else {
            return Err("Native display reported an invalid frame.".into());
        };
        let Some(work) = WindowPlacement::observed(wx, wy, ww, wh) else {
            return Err("Native display reported an invalid work area.".into());
        };
        areas.push(DisplayArea { frame, work });
    }
    Ok(areas)
}
