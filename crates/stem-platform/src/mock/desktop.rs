//! The scripted desktop's windows: what a window action does to them, and
//! a window a test moves.

use stem_core::{PlatformEvent, Rect, WindowAction, WindowId};

use super::{MockCall, MockPlatform};
use crate::{PlatformError, PlatformResult};

impl MockPlatform {
    /// A window moved or resized: give it a new frame (`None` for a
    /// frame the platform could not read) and report it the way the events
    /// thread does after a move, with one `WindowUpdated`. Unknown ids are
    /// ignored, like a move of a window the thread does not track.
    pub fn move_window(&self, id: WindowId, rect: Option<Rect>) {
        let updated = {
            let mut s = self.state.lock();
            match s.windows.iter_mut().find(|w| w.id == id) {
                Some(w) => {
                    w.rect = rect;
                    Some(w.clone())
                }
                None => None,
            }
        };
        if let Some(raw) = updated {
            self.push(PlatformEvent::WindowUpdated(raw));
        }
    }

    /// [`Platform::window_action`](crate::Platform::window_action) on the
    /// scripted windows.
    pub(super) fn act_on_window(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
        self.record(MockCall::WindowAction(id, action));
        let event = {
            let mut s = self.state.lock();
            let Some(idx) = s.windows.iter().position(|w| w.id == id) else {
                return Err(PlatformError::WindowNotFound(id));
            };
            match action {
                WindowAction::Activate => {
                    s.windows[idx].is_minimized = false;
                    s.foreground = Some(id);
                    vec![
                        PlatformEvent::WindowUpdated(s.windows[idx].clone()),
                        PlatformEvent::ForegroundChanged(Some(id)),
                    ]
                }
                WindowAction::Minimize => {
                    s.windows[idx].is_minimized = true;
                    if s.foreground == Some(id) {
                        s.foreground = None;
                    }
                    vec![
                        PlatformEvent::WindowUpdated(s.windows[idx].clone()),
                        PlatformEvent::ForegroundChanged(s.foreground),
                    ]
                }
                WindowAction::Restore => {
                    s.windows[idx].is_minimized = false;
                    vec![PlatformEvent::WindowUpdated(s.windows[idx].clone())]
                }
                WindowAction::Close => {
                    s.windows.remove(idx);
                    if s.foreground == Some(id) {
                        s.foreground = None;
                    }
                    vec![
                        PlatformEvent::WindowRemoved(id),
                        PlatformEvent::ForegroundChanged(s.foreground),
                    ]
                }
            }
        };
        for ev in event {
            self.push(ev);
        }
        Ok(())
    }
}
