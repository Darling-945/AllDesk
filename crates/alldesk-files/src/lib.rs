pub mod manifest;
pub mod transfer;
pub mod validate;

pub use manifest::FileManifest;
pub use transfer::{FileChunk, FileTransfer, ProgressCallback};
pub use validate::{
    compute_crc32, detect_file_type, validate_file_content, validate_file_size, validate_filename,
    verify_crc32, FileCategory, ValidationResult, MAX_FILE_SIZE,
};
