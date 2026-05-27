pub mod transfer;
pub mod manifest;
pub mod validate;

pub use transfer::{FileTransfer, FileChunk, ProgressCallback};
pub use manifest::FileManifest;
pub use validate::{
    validate_filename, validate_file_content, validate_file_size,
    detect_file_type, compute_crc32, verify_crc32,
    ValidationResult, FileCategory, MAX_FILE_SIZE,
};
