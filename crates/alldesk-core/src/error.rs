use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Capture error: {0}")]
    Capture(String),

    #[error("Codec error: {0}")]
    Codec(String),

    #[error("Input error: {0}")]
    Input(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("{0}")]
    Other(String),
}
