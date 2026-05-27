cfg_if::cfg_if! {
    if #[cfg(target_os = "android")] {
        mod android;
        pub use android::{ClipboardContent, ClipboardMonitor};
    } else {
        mod monitor;
        pub use monitor::{ClipboardContent, ClipboardMonitor};
    }
}

pub mod sync;
pub mod sanitize;

pub use sync::ClipboardSync;
pub use sanitize::{sanitize_text, sanitize_image, SanitizeResult};
