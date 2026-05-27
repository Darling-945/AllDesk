//! macOS input controller using CoreGraphics CGEvent API.
//!
//! Requires the calling process to have accessibility permissions:
//! System Settings > Privacy & Security > Accessibility

use std::sync::Mutex;

use alldesk_core::Error;
use core_graphics::event::{
    CGEvent, CGEventType, CGEventTapLocation, CGMouseButton, KeyCode as Vk, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use tracing::{instrument, warn};

use crate::controller::{ButtonState, InputController, KeyCode, KeyState, MouseButton};

pub struct MacInputController {
    source: CGEventSource,
    cursor_pos: Mutex<CGPoint>,
}

impl MacInputController {
    pub fn new() -> Self {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .expect("Failed to create CGEventSource");
        Self {
            source,
            cursor_pos: Mutex::new(CGPoint::new(0.0, 0.0)),
        }
    }
}

impl Default for MacInputController {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for MacInputController {}
unsafe impl Sync for MacInputController {}

fn key_code_to_vk(key: &KeyCode) -> Option<u16> {
    match key {
        KeyCode::Char(c) => char_to_vk(*c),
        KeyCode::Enter => Some(Vk::RETURN),
        KeyCode::Escape => Some(Vk::ESCAPE),
        KeyCode::Tab => Some(Vk::TAB),
        KeyCode::Backspace => Some(Vk::DELETE),
        KeyCode::Delete => Some(Vk::FORWARD_DELETE),
        KeyCode::ArrowUp => Some(Vk::UP_ARROW),
        KeyCode::ArrowDown => Some(Vk::DOWN_ARROW),
        KeyCode::ArrowLeft => Some(Vk::LEFT_ARROW),
        KeyCode::ArrowRight => Some(Vk::RIGHT_ARROW),
        KeyCode::Function(n) => match *n {
            1 => Some(Vk::F1),
            2 => Some(Vk::F2),
            3 => Some(Vk::F3),
            4 => Some(Vk::F4),
            5 => Some(Vk::F5),
            6 => Some(Vk::F6),
            7 => Some(Vk::F7),
            8 => Some(Vk::F8),
            9 => Some(Vk::F9),
            10 => Some(Vk::F10),
            11 => Some(Vk::F11),
            12 => Some(Vk::F12),
            13 => Some(Vk::F13),
            14 => Some(Vk::F14),
            15 => Some(Vk::F15),
            16 => Some(Vk::F16),
            17 => Some(Vk::F17),
            18 => Some(Vk::F18),
            19 => Some(Vk::F19),
            20 => Some(Vk::F20),
            _ => None,
        },
        KeyCode::Unknown(_) => None,
    }
}

fn char_to_vk(c: char) -> Option<u16> {
    match c {
        'a' | 'A' => Some(Vk::ANSI_A),
        'b' | 'B' => Some(Vk::ANSI_B),
        'c' | 'C' => Some(Vk::ANSI_C),
        'd' | 'D' => Some(Vk::ANSI_D),
        'e' | 'E' => Some(Vk::ANSI_E),
        'f' | 'F' => Some(Vk::ANSI_F),
        'g' | 'G' => Some(Vk::ANSI_G),
        'h' | 'H' => Some(Vk::ANSI_H),
        'i' | 'I' => Some(Vk::ANSI_I),
        'j' | 'J' => Some(Vk::ANSI_J),
        'k' | 'K' => Some(Vk::ANSI_K),
        'l' | 'L' => Some(Vk::ANSI_L),
        'm' | 'M' => Some(Vk::ANSI_M),
        'n' | 'N' => Some(Vk::ANSI_N),
        'o' | 'O' => Some(Vk::ANSI_O),
        'p' | 'P' => Some(Vk::ANSI_P),
        'q' | 'Q' => Some(Vk::ANSI_Q),
        'r' | 'R' => Some(Vk::ANSI_R),
        's' | 'S' => Some(Vk::ANSI_S),
        't' | 'T' => Some(Vk::ANSI_T),
        'u' | 'U' => Some(Vk::ANSI_U),
        'v' | 'V' => Some(Vk::ANSI_V),
        'w' | 'W' => Some(Vk::ANSI_W),
        'x' | 'X' => Some(Vk::ANSI_X),
        'y' | 'Y' => Some(Vk::ANSI_Y),
        'z' | 'Z' => Some(Vk::ANSI_Z),
        '0' => Some(Vk::ANSI_0),
        '1' => Some(Vk::ANSI_1),
        '2' => Some(Vk::ANSI_2),
        '3' => Some(Vk::ANSI_3),
        '4' => Some(Vk::ANSI_4),
        '5' => Some(Vk::ANSI_5),
        '6' => Some(Vk::ANSI_6),
        '7' => Some(Vk::ANSI_7),
        '8' => Some(Vk::ANSI_8),
        '9' => Some(Vk::ANSI_9),
        '-' | '_' => Some(Vk::ANSI_MINUS),
        '=' | '+' => Some(Vk::ANSI_EQUAL),
        '[' | '{' => Some(Vk::ANSI_LEFT_BRACKET),
        ']' | '}' => Some(Vk::ANSI_RIGHT_BRACKET),
        '\\' | '|' => Some(Vk::ANSI_BACKSLASH),
        ';' | ':' => Some(Vk::ANSI_SEMICOLON),
        '\'' | '"' => Some(Vk::ANSI_QUOTE),
        '`' | '~' => Some(Vk::ANSI_GRAVE),
        ',' | '<' => Some(Vk::ANSI_COMMA),
        '.' | '>' => Some(Vk::ANSI_PERIOD),
        '/' | '?' => Some(Vk::ANSI_SLASH),
        ' ' => Some(Vk::SPACE),
        _ => None,
    }
}

impl InputController for MacInputController {
    #[instrument(skip(self), level = "debug")]
    fn mouse_move(&self, x: i32, y: i32, relative: bool) -> alldesk_core::Result<()> {
        let point = {
            let mut pos = self.cursor_pos.lock().unwrap();
            if relative {
                pos.x += x as f64;
                pos.y += y as f64;
            } else {
                pos.x = x as f64;
                pos.y = y as f64;
            }
            CGPoint::new(pos.x, pos.y)
        };

        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            CGEventType::MouseMoved,
            point,
            CGMouseButton::Left,
        )
        .map_err(|_| Error::Input("Failed to create mouse move event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_click(&self, button: MouseButton, state: ButtonState) -> alldesk_core::Result<()> {
        let (event_type, cg_button) = match (button, state) {
            (MouseButton::Left, ButtonState::Pressed) => {
                (CGEventType::LeftMouseDown, CGMouseButton::Left)
            }
            (MouseButton::Left, ButtonState::Released) => {
                (CGEventType::LeftMouseUp, CGMouseButton::Left)
            }
            (MouseButton::Right, ButtonState::Pressed) => {
                (CGEventType::RightMouseDown, CGMouseButton::Right)
            }
            (MouseButton::Right, ButtonState::Released) => {
                (CGEventType::RightMouseUp, CGMouseButton::Right)
            }
            (MouseButton::Middle, ButtonState::Pressed) => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center)
            }
            (MouseButton::Middle, ButtonState::Released) => {
                (CGEventType::OtherMouseUp, CGMouseButton::Center)
            }
        };

        let (px, py) = {
            let pos = self.cursor_pos.lock().unwrap();
            (pos.x, pos.y)
        };

        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            event_type,
            CGPoint::new(px, py),
            cg_button,
        )
        .map_err(|_| Error::Input("Failed to create mouse click event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_scroll(&self, delta_x: i32, delta_y: i32) -> alldesk_core::Result<()> {
        if delta_x == 0 && delta_y == 0 {
            return Ok(());
        }

        let event = CGEvent::new_scroll_event(
            self.source.clone(),
            ScrollEventUnit::PIXEL,
            2,
            delta_y,
            delta_x,
            0,
        )
        .map_err(|_| Error::Input("Failed to create scroll event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn key_event(&self, key: KeyCode, state: KeyState) -> alldesk_core::Result<()> {
        let key_down = matches!(state, KeyState::Pressed);

        if let Some(vk) = key_code_to_vk(&key) {
            let event = CGEvent::new_keyboard_event(self.source.clone(), vk, key_down)
                .map_err(|_| Error::Input("Failed to create keyboard event".into()))?;
            event.post(CGEventTapLocation::HID);
        } else {
            match &key {
                KeyCode::Char(c) => {
                    let event = CGEvent::new_keyboard_event(self.source.clone(), 0, key_down)
                        .map_err(|_| {
                            Error::Input("Failed to create Unicode keyboard event".into())
                        })?;
                    event.set_string(&c.to_string());
                    event.post(CGEventTapLocation::HID);
                }
                KeyCode::Unknown(vk_code) => {
                    let event =
                        CGEvent::new_keyboard_event(self.source.clone(), *vk_code as u16, key_down)
                            .map_err(|_| {
                                Error::Input(
                                    "Failed to create keyboard event for unknown VK".into(),
                                )
                            })?;
                    event.post(CGEventTapLocation::HID);
                }
                other => {
                    warn!("Cannot map key code {:?} to macOS virtual key", other);
                    return Err(Error::Input(format!(
                        "Cannot map key code {:?} to macOS virtual key",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn unicode_char(&self, ch: char) -> alldesk_core::Result<()> {
        let event = CGEvent::new_keyboard_event(self.source.clone(), 0, true)
            .map_err(|_| Error::Input("Failed to create Unicode char down event".into()))?;
        event.set_string(&ch.to_string());
        event.post(CGEventTapLocation::HID);

        let event = CGEvent::new_keyboard_event(self.source.clone(), 0, false)
            .map_err(|_| Error::Input("Failed to create Unicode char up event".into()))?;
        event.set_string(&ch.to_string());
        event.post(CGEventTapLocation::HID);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Enter), Some(Vk::RETURN));
        assert_eq!(key_code_to_vk(&KeyCode::Escape), Some(Vk::ESCAPE));
        assert_eq!(key_code_to_vk(&KeyCode::Tab), Some(Vk::TAB));
        assert_eq!(key_code_to_vk(&KeyCode::Backspace), Some(Vk::DELETE));
        assert_eq!(key_code_to_vk(&KeyCode::Delete), Some(Vk::FORWARD_DELETE));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowUp), Some(Vk::UP_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowDown), Some(Vk::DOWN_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowLeft), Some(Vk::LEFT_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowRight), Some(Vk::RIGHT_ARROW));
    }

    #[test]
    fn test_function_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Function(1)), Some(Vk::F1));
        assert_eq!(key_code_to_vk(&KeyCode::Function(12)), Some(Vk::F12));
        assert_eq!(key_code_to_vk(&KeyCode::Function(20)), Some(Vk::F20));
        assert_eq!(key_code_to_vk(&KeyCode::Function(0)), None);
        assert_eq!(key_code_to_vk(&KeyCode::Function(21)), None);
    }

    #[test]
    fn test_ascii_char_mapping() {
        assert_eq!(char_to_vk('a'), Some(Vk::ANSI_A));
        assert_eq!(char_to_vk('Z'), Some(Vk::ANSI_Z));
        assert_eq!(char_to_vk('0'), Some(Vk::ANSI_0));
        assert_eq!(char_to_vk('9'), Some(Vk::ANSI_9));
        assert_eq!(char_to_vk(' '), Some(Vk::SPACE));
    }

    #[test]
    fn test_non_ascii_returns_none() {
        assert_eq!(char_to_vk('\u{4e2d}'), None); // CJK '中'
        assert_eq!(char_to_vk('\u{00e9}'), None); // é
    }

    #[test]
    fn test_punctuation_mapping() {
        assert_eq!(char_to_vk('-'), Some(Vk::ANSI_MINUS));
        assert_eq!(char_to_vk('='), Some(Vk::ANSI_EQUAL));
        assert_eq!(char_to_vk('['), Some(Vk::ANSI_LEFT_BRACKET));
        assert_eq!(char_to_vk(']'), Some(Vk::ANSI_RIGHT_BRACKET));
        assert_eq!(char_to_vk('\\'), Some(Vk::ANSI_BACKSLASH));
        assert_eq!(char_to_vk(';'), Some(Vk::ANSI_SEMICOLON));
        assert_eq!(char_to_vk('\''), Some(Vk::ANSI_QUOTE));
        assert_eq!(char_to_vk('`'), Some(Vk::ANSI_GRAVE));
        assert_eq!(char_to_vk(','), Some(Vk::ANSI_COMMA));
        assert_eq!(char_to_vk('.'), Some(Vk::ANSI_PERIOD));
        assert_eq!(char_to_vk('/'), Some(Vk::ANSI_SLASH));
    }

    #[test]
    fn test_shift_variant_mapping() {
        assert_eq!(char_to_vk('_'), Some(Vk::ANSI_MINUS));
        assert_eq!(char_to_vk('+'), Some(Vk::ANSI_EQUAL));
        assert_eq!(char_to_vk('{'), Some(Vk::ANSI_LEFT_BRACKET));
        assert_eq!(char_to_vk('}'), Some(Vk::ANSI_RIGHT_BRACKET));
        assert_eq!(char_to_vk('|'), Some(Vk::ANSI_BACKSLASH));
        assert_eq!(char_to_vk(':'), Some(Vk::ANSI_SEMICOLON));
        assert_eq!(char_to_vk('"'), Some(Vk::ANSI_QUOTE));
        assert_eq!(char_to_vk('~'), Some(Vk::ANSI_GRAVE));
        assert_eq!(char_to_vk('<'), Some(Vk::ANSI_COMMA));
        assert_eq!(char_to_vk('>'), Some(Vk::ANSI_PERIOD));
        assert_eq!(char_to_vk('?'), Some(Vk::ANSI_SLASH));
    }

    #[test]
    fn test_unknown_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Unknown(42)), None);
    }
}
