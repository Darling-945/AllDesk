pub mod error;
pub mod config;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/alldesk.rs"));
}

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
