pub mod stroke;
pub mod protocol;

pub use stroke::{Stroke, Point};
pub use protocol::{WhiteboardEvent, WhiteboardSync, WhiteboardSnapshot, TimestampedEvent};
