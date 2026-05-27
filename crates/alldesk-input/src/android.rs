//! Android input controller via AccessibilityService (JNI bridge).
//!
//! On Android, input injection requires an AccessibilityService with gesture
//! dispatch capability. Since Rust cannot directly call Android Java/Kotlin
//! APIs, this module buffers input events and exposes them to the Kotlin side
//! through a JNI-callable drain function.
//!
//! Flow:
//! 1. Flutter/Rust generates InputEvents (mouse, key, touch).
//! 2. Events are buffered in AndroidInputController.
//! 3. The Kotlin AccessibilityInputService periodically calls into Rust via
//!    JNI to drain pending events.
//! 4. The Kotlin side uses dispatchGesture() or performGlobalAction() to
//!    inject them into the Android system.

use std::cell::RefCell;
use std::sync::Mutex;

use alldesk_core::Result;

use crate::controller::{
    ButtonState, DisplayRect, InputController, KeyCode, KeyState, MouseButton,
    TouchEvent, TouchPoint,
};

/// Commands that the Kotlin AccessibilityService can execute.
///
/// Each variant maps to a specific Android API call:
/// - Touch events -> dispatchGesture() with GestureDescription
/// - Key events -> performGlobalAction() or dispatchGesture()
/// - Scroll events -> dispatchGesture() with swipe gesture
#[derive(Debug, Clone)]
pub enum InputCommand {
    /// Move pointer to absolute (x, y) position.
    MouseMove { x: i32, y: i32 },
    /// Click or release a mouse button.
    MouseClick { button: MouseButton, state: ButtonState },
    /// Scroll by (delta_x, delta_y) units.
    MouseScroll { delta_x: i32, delta_y: i32 },
    /// Press or release a key.
    Key { key: KeyCode, state: KeyState },
    /// Inject a Unicode character.
    UnicodeChar { ch: char },
    /// Forward a touch gesture event.
    #[allow(dead_code)]
    Touch { event: TouchEvent },
}

/// Android input controller that buffers events for JNI-based injection.
///
/// Uses an internal Vec of InputCommand items. The Kotlin side calls
/// `drain_pending_commands()` via JNI to retrieve and execute them.
pub struct AndroidInputController {
    commands: Mutex<Vec<InputCommand>>,
}

impl AndroidInputController {
    /// Create a new AndroidInputController with an empty command buffer.
    pub fn new() -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
        }
    }

    /// Buffer a single command for the Kotlin side to pick up.
    fn push_command(&self, cmd: InputCommand) {
        if let Ok(mut cmds) = self.commands.lock() {
            cmds.push(cmd);
        }
    }

    /// Drain all pending commands, returning them for the Kotlin side to process.
    ///
    /// This is the primary JNI-callable function. After calling this, the
    /// internal buffer is cleared. The Kotlin AccessibilityInputService should
    /// call this periodically (e.g., on a timer or after each frame) and
    /// execute the returned commands via dispatchGesture() / performGlobalAction().
    pub fn drain_pending_commands(&self) -> Vec<InputCommand> {
        if let Ok(mut cmds) = self.commands.lock() {
            std::mem::take(&mut *cmds)
        } else {
            Vec::new()
        }
    }

    /// Returns the number of pending commands in the buffer.
    pub fn pending_count(&self) -> usize {
        self.commands.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl Default for AndroidInputController {
    fn default() -> Self {
        Self::new()
    }
}

impl InputController for AndroidInputController {
    fn mouse_move(&self, x: i32, y: i32, _relative: bool) -> Result<()> {
        self.push_command(InputCommand::MouseMove { x, y });
        Ok(())
    }

    fn mouse_click(&self, button: MouseButton, state: ButtonState) -> Result<()> {
        self.push_command(InputCommand::MouseClick { button, state });
        Ok(())
    }

    fn mouse_scroll(&self, delta_x: i32, delta_y: i32) -> Result<()> {
        self.push_command(InputCommand::MouseScroll { delta_x, delta_y });
        Ok(())
    }

    fn key_event(&self, key: KeyCode, state: KeyState) -> Result<()> {
        self.push_command(InputCommand::Key { key, state });
        Ok(())
    }

    fn unicode_char(&self, ch: char) -> Result<()> {
        self.push_command(InputCommand::UnicodeChar { ch });
        Ok(())
    }

    fn touch_event(&self, event: TouchEvent) -> Result<()> {
        self.push_command(InputCommand::Touch { event });
        Ok(())
    }

    fn get_displays(&self) -> Vec<DisplayRect> {
        // Android has a single display; actual dimensions come from the
        // capture module at runtime. Use a sensible default.
        vec![DisplayRect {
            x: 0,
            y: 0,
            width: 1080,
            height: 1920,
        }]
    }
}

// Safety: AndroidInputController uses Mutex for interior mutability,
// which is Send + Sync safe.
unsafe impl Send for AndroidInputController {}
unsafe impl Sync for AndroidInputController {}

// ---------------------------------------------------------------------------
// JNI bridge helpers
// ---------------------------------------------------------------------------

thread_local! {
    /// Thread-local controller instance used by the JNI bridge functions.
    /// Set by the Flutter side after the AccessibilityService is connected.
    static JNI_CONTROLLER: RefCell<Option<AndroidInputController>> = RefCell::new(None);
}

/// Initialize the JNI bridge with a fresh controller.
/// Call this from the Flutter side when the AccessibilityService is enabled.
#[allow(dead_code)]
pub fn jni_init_controller() {
    JNI_CONTROLLER.with(|c| {
        *c.borrow_mut() = Some(AndroidInputController::new());
    });
}

/// Drain pending commands from the thread-local JNI controller.
/// Returns an empty Vec if the controller is not initialized.
///
/// The Kotlin side should call this via JNI to retrieve buffered events.
#[allow(dead_code)]
pub fn jni_drain_commands() -> Vec<InputCommand> {
    JNI_CONTROLLER.with(|c| {
        if let Some(ref ctrl) = *c.borrow() {
            ctrl.drain_pending_commands()
        } else {
            Vec::new()
        }
    })
}

/// Inject an input event into the thread-local JNI controller.
/// Returns false if the controller is not initialized.
#[allow(dead_code)]
pub fn jni_inject_mouse_move(x: i32, y: i32) -> bool {
    JNI_CONTROLLER.with(|c| {
        if let Some(ref ctrl) = *c.borrow() {
            ctrl.mouse_move(x, y, false).is_ok()
        } else {
            false
        }
    })
}

/// Inject a click event into the thread-local JNI controller.
#[allow(dead_code)]
pub fn jni_inject_mouse_click(button: MouseButton, state: ButtonState) -> bool {
    JNI_CONTROLLER.with(|c| {
        if let Some(ref ctrl) = *c.borrow() {
            ctrl.mouse_click(button, state).is_ok()
        } else {
            false
        }
    })
}

/// Inject a touch event into the thread-local JNI controller.
#[allow(dead_code)]
pub fn jni_inject_touch(event: TouchEvent) -> bool {
    JNI_CONTROLLER.with(|c| {
        if let Some(ref ctrl) = *c.borrow() {
            ctrl.touch_event(event).is_ok()
        } else {
            false
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_creation() {
        let ctrl = AndroidInputController::new();
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_event_buffering_and_draining() {
        let ctrl = AndroidInputController::new();

        // Buffer several events
        ctrl.mouse_move(100, 200, false).unwrap();
        ctrl.mouse_click(MouseButton::Left, ButtonState::Pressed).unwrap();
        ctrl.mouse_click(MouseButton::Left, ButtonState::Released).unwrap();

        assert_eq!(ctrl.pending_count(), 3);

        // Drain all events
        let cmds = ctrl.drain_pending_commands();
        assert_eq!(cmds.len(), 3);
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_inject_event_stores_events() {
        let ctrl = AndroidInputController::new();

        ctrl.mouse_move(50, 75, false).unwrap();
        ctrl.key_event(KeyCode::Enter, KeyState::Pressed).unwrap();
        ctrl.unicode_char('A').unwrap();

        let cmds = ctrl.drain_pending_commands();
        assert_eq!(cmds.len(), 3);

        assert!(matches!(cmds[0], InputCommand::MouseMove { x: 50, y: 75 }));
        assert!(matches!(cmds[1], InputCommand::Key { key: KeyCode::Enter, state: KeyState::Pressed }));
        assert!(matches!(cmds[2], InputCommand::UnicodeChar { ch: 'A' }));
    }

    #[test]
    fn test_drain_clears_buffer() {
        let ctrl = AndroidInputController::new();

        ctrl.mouse_scroll(0, 3).unwrap();
        ctrl.mouse_scroll(0, -2).unwrap();
        assert_eq!(ctrl.pending_count(), 2);

        let first = ctrl.drain_pending_commands();
        assert_eq!(first.len(), 2);

        // Buffer should be empty after drain
        let second = ctrl.drain_pending_commands();
        assert!(second.is_empty());
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_multiple_event_types() {
        let ctrl = AndroidInputController::new();

        // Mouse events
        ctrl.mouse_move(10, 20, false).unwrap();
        ctrl.mouse_click(MouseButton::Right, ButtonState::Pressed).unwrap();
        ctrl.mouse_scroll(-1, 5).unwrap();

        // Key events
        ctrl.key_event(KeyCode::Char('a'), KeyState::Pressed).unwrap();
        ctrl.key_event(KeyCode::Function(1), KeyState::Released).unwrap();
        ctrl.unicode_char('\u{4e2d}').unwrap();

        // Touch events
        let touch = TouchEvent::Down {
            points: vec![TouchPoint { id: 0, x: 100.0, y: 200.0, pressure: 1.0 }],
        };
        ctrl.touch_event(touch).unwrap();

        let cmds = ctrl.drain_pending_commands();
        assert_eq!(cmds.len(), 7);

        // Verify each type
        assert!(matches!(&cmds[0], InputCommand::MouseMove { x: 10, y: 20 }));
        assert!(matches!(&cmds[1], InputCommand::MouseClick { button: MouseButton::Right, state: ButtonState::Pressed }));
        assert!(matches!(&cmds[2], InputCommand::MouseScroll { delta_x: -1, delta_y: 5 }));
        assert!(matches!(&cmds[3], InputCommand::Key { key: KeyCode::Char('a'), state: KeyState::Pressed }));
        assert!(matches!(&cmds[4], InputCommand::Key { key: KeyCode::Function(1), state: KeyState::Released }));
        assert!(matches!(&cmds[5], InputCommand::UnicodeChar { ch: '\u{4e2d}' }));
        assert!(matches!(&cmds[6], InputCommand::Touch { .. }));
    }

    #[test]
    fn test_get_displays_returns_android_default() {
        let ctrl = AndroidInputController::new();
        let displays = ctrl.get_displays();
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].x, 0);
        assert_eq!(displays[0].y, 0);
        // Android default is portrait orientation
        assert_eq!(displays[0].width, 1080);
        assert_eq!(displays[0].height, 1920);
    }

    #[test]
    fn test_default_trait() {
        let ctrl = AndroidInputController::default();
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_touch_event_directly() {
        let ctrl = AndroidInputController::new();

        // Touch down
        let down = TouchEvent::Down {
            points: vec![
                TouchPoint { id: 0, x: 100.0, y: 200.0, pressure: 0.8 },
                TouchPoint { id: 1, x: 300.0, y: 400.0, pressure: 0.6 },
            ],
        };
        ctrl.touch_event(down).unwrap();

        // Touch move
        let mv = TouchEvent::Move {
            points: vec![TouchPoint { id: 0, x: 110.0, y: 210.0, pressure: 0.8 }],
        };
        ctrl.touch_event(mv).unwrap();

        // Touch up
        let up = TouchEvent::Up {
            points: vec![TouchPoint { id: 0, x: 110.0, y: 210.0, pressure: 0.0 }],
        };
        ctrl.touch_event(up).unwrap();

        let cmds = ctrl.drain_pending_commands();
        assert_eq!(cmds.len(), 3);

        for cmd in &cmds {
            assert!(matches!(cmd, InputCommand::Touch { .. }));
        }
    }
}
