//! Windows input controller using SendInput API.

use alldesk_core::Error;
use tracing::{instrument, warn};

use crate::controller::{
    ButtonState, InputController, KeyCode, KeyState, MouseButton,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, KEYBDINPUT, MOUSE_EVENT_FLAGS, MOUSEINPUT,
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_PACKET, VK_RETURN,
    VK_RIGHT, VK_TAB, VK_UP, KEYBD_EVENT_FLAGS, INPUT_TYPE,
};

use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN};

/// Windows input controller that uses the SendInput API for mouse and keyboard
/// injection, including KEYEVENTF_UNICODE for Unicode character input.
pub struct WindowsInputController;

impl WindowsInputController {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsInputController {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper: send a slice of INPUT events via SendInput.
/// Returns the number of events successfully inserted, or an error.
fn send_inputs(inputs: &[INPUT]) -> Result<u32, Error> {
    let sent = unsafe {
        windows::Win32::UI::Input::KeyboardAndMouse::SendInput(inputs, std::mem::size_of::<INPUT>() as i32)
    };
    if sent == 0 {
        Err(Error::Input(format!(
            "SendInput failed: events sent=0, requested={}",
            inputs.len()
        )))
    } else {
        Ok(sent)
    }
}

/// Build a mouse INPUT event.
fn mouse_input(dx: i32, dy: i32, mouse_data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_TYPE(INPUT_MOUSE.0),
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Build a keyboard INPUT event.
fn keyboard_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_TYPE(INPUT_KEYBOARD.0),
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Encode a char into UTF-16 code units.
/// Returns (first_unit, optional_second_unit). BMP characters yield one u16;
/// supplementary characters yield a surrogate pair (two u16 values).
fn char_to_utf16_surrogates(ch: char) -> (u16, Option<u16>) {
    let mut buf = [0u16; 2];
    let s = ch.encode_utf16(&mut buf);
    if s.len() == 1 {
        (s[0], None)
    } else {
        (s[0], Some(s[1]))
    }
}

/// Map a KeyCode to a Windows virtual key code.
fn key_code_to_vk(key: &KeyCode) -> Option<VIRTUAL_KEY> {
    match key {
        KeyCode::Char(c) => {
            let ch = *c as u32;
            // ASCII uppercase letters have direct VK codes (0x41-0x5A)
            if c.is_ascii_uppercase() {
                Some(VIRTUAL_KEY(ch as u16))
            }
            // ASCII lowercase letters map to uppercase VK codes
            else if c.is_ascii_lowercase() {
                let vk = ch - 32; // 'a' -> 'A'
                Some(VIRTUAL_KEY(vk as u16))
            }
            // ASCII digits have direct VK codes (0x30-0x39)
            else if c.is_ascii_digit() {
                Some(VIRTUAL_KEY(ch as u16))
            }
            // For other characters, use the Unicode scan code path
            else {
                None
            }
        }
        KeyCode::Enter => Some(VK_RETURN),
        KeyCode::Escape => Some(VK_ESCAPE),
        KeyCode::Tab => Some(VK_TAB),
        KeyCode::Backspace => Some(VK_BACK),
        KeyCode::Delete => Some(VK_DELETE),
        KeyCode::ArrowUp => Some(VK_UP),
        KeyCode::ArrowDown => Some(VK_DOWN),
        KeyCode::ArrowLeft => Some(VK_LEFT),
        KeyCode::ArrowRight => Some(VK_RIGHT),
        KeyCode::Function(n) => {
            // VK_F1 (0x70) through VK_F24 (0x87)
            if *n >= 1 && *n <= 24 {
                Some(VIRTUAL_KEY(0x70 + *n as u16 - 1))
            } else {
                None
            }
        }
        KeyCode::Unknown(_) => None,
    }
}

impl InputController for WindowsInputController {
    #[instrument(skip(self), level = "debug")]
    fn mouse_move(&self, x: i32, y: i32, relative: bool) -> alldesk_core::Result<()> {
        let flags = if relative {
            MOUSEEVENTF_MOVE
        } else {
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE
        };

        // For absolute positioning, convert pixel coordinates to normalized
        // coordinates (0..65535) as required by MOUSEEVENTF_ABSOLUTE.
        let (dx, dy) = if !relative {
            let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as i64;
            let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as i64;
            let norm_x = if screen_w > 0 { ((x as i64 * 65535) / screen_w) as i32 } else { x };
            let norm_y = if screen_h > 0 { ((y as i64 * 65535) / screen_h) as i32 } else { y };
            (norm_x, norm_y)
        } else {
            (x, y)
        };

        let input = mouse_input(dx, dy, 0, flags);
        send_inputs(&[input])?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_click(&self, button: MouseButton, state: ButtonState) -> alldesk_core::Result<()> {
        let flags = match (button, state) {
            (MouseButton::Left, ButtonState::Pressed) => MOUSEEVENTF_LEFTDOWN,
            (MouseButton::Left, ButtonState::Released) => MOUSEEVENTF_LEFTUP,
            (MouseButton::Right, ButtonState::Pressed) => MOUSEEVENTF_RIGHTDOWN,
            (MouseButton::Right, ButtonState::Released) => MOUSEEVENTF_RIGHTUP,
            (MouseButton::Middle, ButtonState::Pressed) => MOUSEEVENTF_MIDDLEDOWN,
            (MouseButton::Middle, ButtonState::Released) => MOUSEEVENTF_MIDDLEUP,
        };

        let input = mouse_input(0, 0, 0, flags);
        send_inputs(&[input])?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_scroll(&self, delta_x: i32, delta_y: i32) -> alldesk_core::Result<()> {
        let mut inputs = Vec::with_capacity(2);

        // Vertical scroll: positive = up, negative = down.
        // WHEEL_DELTA is 120; we treat delta_y in units of WHEEL_DELTA.
        if delta_y != 0 {
            let input = mouse_input(0, 0, delta_y as u32, MOUSEEVENTF_WHEEL);
            inputs.push(input);
        }

        // Horizontal scroll: positive = right, negative = left.
        if delta_x != 0 {
            let input = mouse_input(0, 0, delta_x as u32, MOUSEEVENTF_HWHEEL);
            inputs.push(input);
        }

        if !inputs.is_empty() {
            send_inputs(&inputs)?;
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn key_event(&self, key: KeyCode, state: KeyState) -> alldesk_core::Result<()> {
        let flags = match state {
            KeyState::Pressed => KEYBD_EVENT_FLAGS(0),
            KeyState::Released => KEYEVENTF_KEYUP,
        };

        if let Some(vk) = key_code_to_vk(&key) {
            let input = keyboard_input(vk, 0, flags);
            send_inputs(&[input])?;
        } else {
            // For characters that don't have a direct VK mapping, use KEYEVENTF_UNICODE
            // with the scan code set to the Unicode code point.
            match &key {
                KeyCode::Char(c) => {
                    let (first, second) = char_to_utf16_surrogates(*c);
                    let input = keyboard_input(VIRTUAL_KEY(0), first, KEYEVENTF_UNICODE | flags);
                    send_inputs(&[input])?;
                    if let Some(second_unit) = second {
                        let input = keyboard_input(VIRTUAL_KEY(0), second_unit, KEYEVENTF_UNICODE | flags);
                        send_inputs(&[input])?;
                    }
                }
                KeyCode::Unknown(vk_code) => {
                    let input = keyboard_input(VIRTUAL_KEY(*vk_code as u16), 0, flags);
                    send_inputs(&[input])?;
                }
                other => {
                    warn!("Cannot map key code {:?} to virtual key", other);
                    return Err(Error::Input(format!(
                        "Cannot map key code {:?} to Windows virtual key",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn unicode_char(&self, ch: char) -> alldesk_core::Result<()> {
        // Use VK_PACKET via SendInput for Unicode character injection.
        // Handle supplementary characters via UTF-16 surrogate pairs.
        let (first, second) = char_to_utf16_surrogates(ch);

        // Key down
        let input_down = keyboard_input(
            VK_PACKET,
            first,
            KEYEVENTF_UNICODE,
        );
        send_inputs(&[input_down])?;

        // Key up
        let input_up = keyboard_input(
            VK_PACKET,
            first,
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
        );
        send_inputs(&[input_up])?;

        // If this is a supplementary character, send the low surrogate as well.
        if let Some(low) = second {
            let input_down = keyboard_input(
                VK_PACKET,
                low,
                KEYEVENTF_UNICODE,
            );
            send_inputs(&[input_down])?;

            let input_up = keyboard_input(
                VK_PACKET,
                low,
                KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
            );
            send_inputs(&[input_up])?;
        }

        Ok(())
    }

    fn get_displays(&self) -> Vec<crate::controller::DisplayRect> {
        use crate::controller::DisplayRect;
        let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };

        // Return the virtual screen as a single combined display.
        // For individual monitor enumeration, the capture module's enumerate_monitors
        // provides per-monitor rects. This gives the bounding virtual screen.
        vec![DisplayRect {
            x,
            y,
            width: w as u32,
            height: h as u32,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Enter), Some(VK_RETURN));
        assert_eq!(key_code_to_vk(&KeyCode::Escape), Some(VK_ESCAPE));
        assert_eq!(key_code_to_vk(&KeyCode::Tab), Some(VK_TAB));
        assert_eq!(key_code_to_vk(&KeyCode::Backspace), Some(VK_BACK));
        assert_eq!(key_code_to_vk(&KeyCode::Delete), Some(VK_DELETE));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowUp), Some(VK_UP));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowDown), Some(VK_DOWN));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowLeft), Some(VK_LEFT));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowRight), Some(VK_RIGHT));
    }

    #[test]
    fn test_function_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Function(1)), Some(VIRTUAL_KEY(0x70)));
        assert_eq!(key_code_to_vk(&KeyCode::Function(12)), Some(VIRTUAL_KEY(0x7B)));
        assert_eq!(key_code_to_vk(&KeyCode::Function(24)), Some(VIRTUAL_KEY(0x87)));
        assert_eq!(key_code_to_vk(&KeyCode::Function(0)), None);
        assert_eq!(key_code_to_vk(&KeyCode::Function(25)), None);
    }

    #[test]
    fn test_char_key_mapping_ascii() {
        assert_eq!(key_code_to_vk(&KeyCode::Char('A')), Some(VIRTUAL_KEY(0x41)));
        assert_eq!(key_code_to_vk(&KeyCode::Char('Z')), Some(VIRTUAL_KEY(0x5A)));
        assert_eq!(key_code_to_vk(&KeyCode::Char('0')), Some(VIRTUAL_KEY(0x30)));
        assert_eq!(key_code_to_vk(&KeyCode::Char('9')), Some(VIRTUAL_KEY(0x39)));
        // Lowercase ASCII letters should map to uppercase VK codes
        assert_eq!(key_code_to_vk(&KeyCode::Char('a')), Some(VIRTUAL_KEY(0x41)));
        assert_eq!(key_code_to_vk(&KeyCode::Char('z')), Some(VIRTUAL_KEY(0x5A)));
    }

    #[test]
    fn test_char_to_utf16_surrogates_bmp() {
        // BMP characters should have a single u16, no surrogate
        let (first, second) = char_to_utf16_surrogates('\u{4e2d}'); // CJK '中'
        assert_eq!(first, 0x4e2d);
        assert_eq!(second, None);

        let (first, second) = char_to_utf16_surrogates('A');
        assert_eq!(first, 0x41);
        assert_eq!(second, None);
    }

    #[test]
    fn test_char_to_utf16_surrogates_supplementary() {
        // Supplementary character U+1F600 (😀) should produce a surrogate pair
        let (first, second) = char_to_utf16_surrogates('\u{1F600}');
        assert!(first >= 0xD800 && first <= 0xDBFF); // high surrogate
        let low = second.expect("supplementary char should have low surrogate");
        assert!(low >= 0xDC00 && low <= 0xDFFF); // low surrogate
    }

    #[test]
    fn test_char_key_mapping_unicode() {
        // Non-ASCII characters should return None (will use KEYEVENTF_UNICODE path)
        assert_eq!(key_code_to_vk(&KeyCode::Char('\u{4e2d}')), None);
        assert_eq!(key_code_to_vk(&KeyCode::Char('\u{00e9}')), None);
    }

    #[test]
    fn test_controller_creation() {
        let _controller = WindowsInputController::new();
        let _controller = WindowsInputController::default();
    }

    #[test]
    fn test_mouse_input_builder() {
        let input = mouse_input(100, 200, 0, MOUSEEVENTF_MOVE);
        assert_eq!(input.r#type.0, INPUT_MOUSE.0);
        unsafe {
            assert_eq!(input.Anonymous.mi.dx, 100);
            assert_eq!(input.Anonymous.mi.dy, 200);
        }
    }

    #[test]
    fn test_keyboard_input_builder() {
        let input = keyboard_input(VK_RETURN, 0, KEYEVENTF_KEYUP);
        assert_eq!(input.r#type.0, INPUT_KEYBOARD.0);
        unsafe {
            assert_eq!(input.Anonymous.ki.wVk.0, VK_RETURN.0);
            assert_eq!(input.Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
        }
    }
}
