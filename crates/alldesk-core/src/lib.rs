pub mod error;
pub mod config;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
