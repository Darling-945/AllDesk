use alldesk_core::Result;

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy)]
pub enum KeyCode {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Function(u8),
    Unknown(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// A touch event for mobile/remote input forwarding.
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    /// Unique pointer/finger ID.
    pub id: u32,
    /// X coordinate in pixels (absolute).
    pub x: f64,
    /// Y coordinate in pixels (absolute).
    pub y: f64,
    /// Touch pressure (0.0 = none, 1.0 = full).
    pub pressure: f64,
}

/// Touch event type for gesture and multi-touch support.
#[derive(Debug, Clone)]
pub enum TouchEvent {
    /// One or more fingers touched down.
    Down { points: Vec<TouchPoint> },
    /// One or more fingers moved.
    Move { points: Vec<TouchPoint> },
    /// One or more fingers lifted.
    Up { points: Vec<TouchPoint> },
    /// All fingers lifted (convenience event).
    Cancel { points: Vec<TouchPoint> },
}

/// Monitor geometry for multi-display coordinate mapping.
#[derive(Debug, Clone, Copy)]
pub struct DisplayRect {
    /// Left edge in virtual screen coordinates.
    pub x: i32,
    /// Top edge in virtual screen coordinates.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

pub trait InputController: Send + Sync {
    fn mouse_move(&self, x: i32, y: i32, relative: bool) -> Result<()>;
    fn mouse_click(&self, button: MouseButton, state: ButtonState) -> Result<()>;
    fn mouse_scroll(&self, delta_x: i32, delta_y: i32) -> Result<()>;
    fn key_event(&self, key: KeyCode, state: KeyState) -> Result<()>;
    fn unicode_char(&self, ch: char) -> Result<()>;

    /// Forward a touch event to the OS input system.
    /// Default implementation converts touch to mouse events (single-touch fallback).
    fn touch_event(&self, event: TouchEvent) -> Result<()> {
        // Default: convert first touch point to a mouse move/click.
        match event {
            TouchEvent::Down { ref points } | TouchEvent::Move { ref points } => {
                if let Some(p) = points.first() {
                    self.mouse_move(p.x as i32, p.y as i32, false)?;
                    if let TouchEvent::Down { .. } = event {
                        self.mouse_click(MouseButton::Left, ButtonState::Pressed)?;
                    }
                }
            }
            TouchEvent::Up { .. } | TouchEvent::Cancel { .. } => {
                self.mouse_click(MouseButton::Left, ButtonState::Released)?;
            }
        }
        Ok(())
    }

    /// Get the current display layout for multi-monitor coordinate mapping.
    /// Returns the primary display by default; platform implementations should override.
    fn get_displays(&self) -> Vec<DisplayRect> {
        vec![DisplayRect { x: 0, y: 0, width: 1920, height: 1080 }]
    }

    /// Map normalized coordinates (0.0-1.0) to absolute pixel coordinates
    /// accounting for multi-monitor layout.
    fn map_to_display(&self, norm_x: f64, norm_y: f64, display_index: u32) -> (i32, i32) {
        let displays = self.get_displays();
        let display = displays.get(display_index as usize)
            .or_else(|| displays.first())
            .unwrap_or(&DisplayRect { x: 0, y: 0, width: 1920, height: 1080 });

        let abs_x = display.x + (norm_x * display.width as f64) as i32;
        let abs_y = display.y + (norm_y * display.height as f64) as i32;
        (abs_x, abs_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_point_fields() {
        let tp = TouchPoint { id: 0, x: 100.0, y: 200.0, pressure: 0.5 };
        assert_eq!(tp.id, 0);
        assert!((tp.x - 100.0).abs() < f64::EPSILON);
        assert!((tp.pressure - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_touch_event_variants() {
        let down = TouchEvent::Down {
            points: vec![TouchPoint { id: 0, x: 10.0, y: 20.0, pressure: 1.0 }],
        };
        let up = TouchEvent::Up {
            points: vec![TouchPoint { id: 0, x: 10.0, y: 20.0, pressure: 0.0 }],
        };
        let cancel = TouchEvent::Cancel {
            points: vec![TouchPoint { id: 0, x: 10.0, y: 20.0, pressure: 0.0 }],
        };

        match down { TouchEvent::Down { .. } => {}, _ => panic!("expected Down") }
        match up { TouchEvent::Up { .. } => {}, _ => panic!("expected Up") }
        match cancel { TouchEvent::Cancel { .. } => {}, _ => panic!("expected Cancel") }
    }

    #[test]
    fn test_multi_touch_event() {
        let points = vec![
            TouchPoint { id: 0, x: 100.0, y: 200.0, pressure: 0.8 },
            TouchPoint { id: 1, x: 300.0, y: 400.0, pressure: 0.6 },
        ];
        let move_event = TouchEvent::Move { points };
        if let TouchEvent::Move { points: ref p } = move_event {
            assert_eq!(p.len(), 2);
            assert_eq!(p[0].id, 0);
            assert_eq!(p[1].id, 1);
        } else {
            panic!("expected Move");
        }
    }

    #[test]
    fn test_display_rect_fields() {
        let rect = DisplayRect { x: -1920, y: 0, width: 1920, height: 1080 };
        assert_eq!(rect.x, -1920);
        assert_eq!(rect.width, 1920);
    }

    #[test]
    fn test_map_to_display_primary() {
        struct TestController;
        impl InputController for TestController {
            fn mouse_move(&self, _x: i32, _y: i32, _relative: bool) -> Result<()> { Ok(()) }
            fn mouse_click(&self, _button: MouseButton, _state: ButtonState) -> Result<()> { Ok(()) }
            fn mouse_scroll(&self, _delta_x: i32, _delta_y: i32) -> Result<()> { Ok(()) }
            fn key_event(&self, _key: KeyCode, _state: KeyState) -> Result<()> { Ok(()) }
            fn unicode_char(&self, _ch: char) -> Result<()> { Ok(()) }
            fn get_displays(&self) -> Vec<DisplayRect> {
                vec![
                    DisplayRect { x: 0, y: 0, width: 1920, height: 1080 },
                    DisplayRect { x: 1920, y: 0, width: 1920, height: 1080 },
                ]
            }
        }

        let ctrl = TestController;
        // Primary display (index 0)
        let (x, y) = ctrl.map_to_display(0.5, 0.5, 0);
        assert_eq!(x, 960);
        assert_eq!(y, 540);

        // Second display (index 1) — offset by 1920px
        let (x, y) = ctrl.map_to_display(0.5, 0.5, 1);
        assert_eq!(x, 1920 + 960);
        assert_eq!(y, 540);

        // Top-left corner of primary
        let (x, y) = ctrl.map_to_display(0.0, 0.0, 0);
        assert_eq!(x, 0);
        assert_eq!(y, 0);

        // Bottom-right corner of primary
        let (x, y) = ctrl.map_to_display(1.0, 1.0, 0);
        assert_eq!(x, 1920);
        assert_eq!(y, 1080);
    }

    #[test]
    fn test_map_to_display_invalid_index_falls_back() {
        struct SingleDisplayController;
        impl InputController for SingleDisplayController {
            fn mouse_move(&self, _x: i32, _y: i32, _relative: bool) -> Result<()> { Ok(()) }
            fn mouse_click(&self, _button: MouseButton, _state: ButtonState) -> Result<()> { Ok(()) }
            fn mouse_scroll(&self, _delta_x: i32, _delta_y: i32) -> Result<()> { Ok(()) }
            fn key_event(&self, _key: KeyCode, _state: KeyState) -> Result<()> { Ok(()) }
            fn unicode_char(&self, _ch: char) -> Result<()> { Ok(()) }
            fn get_displays(&self) -> Vec<DisplayRect> {
                vec![DisplayRect { x: 0, y: 0, width: 1920, height: 1080 }]
            }
        }

        let ctrl = SingleDisplayController;
        // Invalid index falls back to first display
        let (x, y) = ctrl.map_to_display(0.5, 0.5, 99);
        assert_eq!(x, 960);
        assert_eq!(y, 540);
    }
}
