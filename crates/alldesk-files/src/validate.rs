//! File content validation for secure file transfers.
//!
//! Validates transferred files for integrity, size limits, and dangerous
//! content patterns (e.g., executables, scripts with suspicious payloads).

use alldesk_core::error::Error;
use alldesk_core::Result;
use std::path::Path;

/// Maximum allowed file size (4 GB).
pub const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Maximum allowed filename length.
pub const MAX_FILENAME_LEN: usize = 255;

/// File type classification based on magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Executable,
    Script,
    Unknown,
}

/// Result of file validation.
#[derive(Debug)]
pub struct ValidationResult {
    /// Whether the file passed all checks.
    pub is_safe: bool,
    /// Detected file category.
    pub category: FileCategory,
    /// List of warnings (file is still allowed but flagged).
    pub warnings: Vec<String>,
    /// List of blocking issues (file is rejected).
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn safe(category: FileCategory) -> Self {
        Self {
            is_safe: true,
            category,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn unsafe_file(category: FileCategory, errors: Vec<String>) -> Self {
        Self {
            is_safe: false,
            category,
            warnings: Vec::new(),
            errors,
        }
    }
}

/// Known magic byte signatures for file type detection.
struct MagicSignature {
    offset: usize,
    bytes: &'static [u8],
    category: FileCategory,
    name: &'static str,
}

const MAGIC_SIGNATURES: &[MagicSignature] = &[
    // Images
    MagicSignature {
        offset: 0,
        bytes: b"\x89PNG\r\n\x1a\n",
        category: FileCategory::Image,
        name: "PNG",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\xFF\xD8\xFF",
        category: FileCategory::Image,
        name: "JPEG",
    },
    MagicSignature {
        offset: 0,
        bytes: b"GIF87a",
        category: FileCategory::Image,
        name: "GIF87a",
    },
    MagicSignature {
        offset: 0,
        bytes: b"GIF89a",
        category: FileCategory::Image,
        name: "GIF89a",
    },
    MagicSignature {
        offset: 0,
        bytes: b"BM",
        category: FileCategory::Image,
        name: "BMP",
    },
    MagicSignature {
        offset: 0,
        bytes: b"RIFF",
        category: FileCategory::Image,
        name: "WebP/RIFF",
    },
    // Video
    MagicSignature {
        offset: 0,
        bytes: b"\x1a\x45\xdf\xa3",
        category: FileCategory::Video,
        name: "MKV/WebM",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x00\x00\x00\x1c\x66\x74\x79\x70",
        category: FileCategory::Video,
        name: "MP4",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x00\x00\x00\x20\x66\x74\x79\x70",
        category: FileCategory::Video,
        name: "MP4",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x00\x00\x00\x18\x66\x74\x79\x70",
        category: FileCategory::Video,
        name: "MP4",
    },
    // Audio
    MagicSignature {
        offset: 0,
        bytes: b"OggS",
        category: FileCategory::Audio,
        name: "OGG",
    },
    MagicSignature {
        offset: 0,
        bytes: b"fLaC",
        category: FileCategory::Audio,
        name: "FLAC",
    },
    MagicSignature {
        offset: 0,
        bytes: b"ID3",
        category: FileCategory::Audio,
        name: "MP3/ID3",
    },
    // Archives
    MagicSignature {
        offset: 0,
        bytes: b"PK\x03\x04",
        category: FileCategory::Archive,
        name: "ZIP",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x1f\x8b",
        category: FileCategory::Archive,
        name: "GZIP",
    },
    MagicSignature {
        offset: 0,
        bytes: b"BZh",
        category: FileCategory::Archive,
        name: "BZIP2",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\xfd7zXZ\x00",
        category: FileCategory::Archive,
        name: "XZ",
    },
    MagicSignature {
        offset: 0,
        bytes: b"Rar!\x1a\x07",
        category: FileCategory::Archive,
        name: "RAR",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x37\x7a\xbc\xaf\x27\x1c",
        category: FileCategory::Archive,
        name: "7Z",
    },
    // Documents
    MagicSignature {
        offset: 0,
        bytes: b"%PDF",
        category: FileCategory::Document,
        name: "PDF",
    },
    // Executables
    MagicSignature {
        offset: 0,
        bytes: b"MZ",
        category: FileCategory::Executable,
        name: "EXE/PE",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\x7fELF",
        category: FileCategory::Executable,
        name: "ELF",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\xfe\xed\xfa",
        category: FileCategory::Executable,
        name: "Mach-O",
    },
    MagicSignature {
        offset: 0,
        bytes: b"\xce\xfa\xed\xfe",
        category: FileCategory::Executable,
        name: "Mach-O",
    },
];

/// Detect file category from magic bytes (first few bytes of the file).
pub fn detect_file_type(data: &[u8]) -> (FileCategory, Option<&'static str>) {
    for sig in MAGIC_SIGNATURES {
        if data.len() >= sig.offset + sig.bytes.len()
            && &data[sig.offset..sig.offset + sig.bytes.len()] == sig.bytes
        {
            return (sig.category, Some(sig.name));
        }
    }
    (FileCategory::Unknown, None)
}

/// Dangerous filename patterns that should be blocked.
const BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "com", "msi", "scr", "pif", "vbs", "vbe", "js", "jse", "wsh", "wsf",
    "ps1", "psm1", "psc1", "sh", "bash", "zsh", "fish", "dll", "so", "dylib", "sys", "drv", "reg",
    "inf",
];

/// Double-extension attack patterns (e.g., "file.txt.exe").
const DOUBLE_EXT_TRICKS: &[&str] = &[
    "txt.exe", "pdf.exe", "jpg.exe", "png.exe", "doc.exe", "txt.scr", "pdf.scr", "jpg.scr",
    "txt.vbs", "pdf.vbs", "txt.js", "pdf.js", "txt.bat", "pdf.bat", "jpg.bat", "txt.cmd",
    "pdf.cmd", "txt.ps1", "pdf.ps1",
];

/// Validate a filename for security issues.
pub fn validate_filename(filename: &str) -> Result<ValidationResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check filename length.
    if filename.len() > MAX_FILENAME_LEN {
        errors.push(format!(
            "Filename too long: {} chars (max {})",
            filename.len(),
            MAX_FILENAME_LEN
        ));
    }

    // Check for path traversal.
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        errors.push("Filename contains path traversal characters".to_string());
    }

    // Check for null bytes.
    if filename.contains('\0') {
        errors.push("Filename contains null bytes".to_string());
    }

    // Check for leading dots (hidden files on Unix).
    if filename.starts_with('.') {
        warnings.push("Hidden file (starts with .)".to_string());
    }

    // Extract extension.
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    if let Some(ref ext) = ext {
        if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
            errors.push(format!("Blocked file extension: .{}", ext));
        }
    }

    // Check for double-extension attacks.
    let lower = filename.to_lowercase();
    for trick in DOUBLE_EXT_TRICKS {
        if lower.ends_with(trick) {
            errors.push(format!("Suspected double-extension attack: {}", trick));
        }
    }

    let is_safe = errors.is_empty();
    Ok(ValidationResult {
        is_safe,
        category: FileCategory::Unknown,
        warnings,
        errors,
    })
}

/// Validate file content (first bytes) for security.
pub fn validate_file_content(data: &[u8], filename: &str) -> Result<ValidationResult> {
    let (category, type_name) = if data.is_empty() {
        (FileCategory::Unknown, None)
    } else {
        detect_file_type(data)
    };

    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Flag executables.
    if category == FileCategory::Executable {
        if let Some(name) = type_name {
            errors.push(format!("Executable file detected: {}", name));
        }
    }

    // Check extension vs content mismatch.
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    if let (Some(ref ext), Some(type_name)) = (&ext, type_name) {
        let mismatch = match (ext.as_str(), category) {
            ("png", FileCategory::Image) => false,
            ("jpg" | "jpeg", FileCategory::Image) => false,
            ("gif", FileCategory::Image) => false,
            ("bmp", FileCategory::Image) => false,
            ("webp", FileCategory::Image) => false,
            ("mp4" | "mkv" | "webm" | "avi", FileCategory::Video) => false,
            ("mp3" | "ogg" | "flac" | "wav", FileCategory::Audio) => false,
            ("zip" | "gz" | "bz2" | "xz" | "rar" | "7z", FileCategory::Archive) => false,
            ("pdf", FileCategory::Document) => false,
            _ => {
                // Allow Unknown to Unknown, warn on others.
                category != FileCategory::Unknown
            }
        };
        if mismatch {
            warnings.push(format!(
                "Extension .{} doesn't match detected type {}",
                ext, type_name
            ));
        }
    }

    // Validate filename too.
    let fn_result = validate_filename(filename)?;
    warnings.extend(fn_result.warnings);
    errors.extend(fn_result.errors);

    let is_safe = errors.is_empty();
    Ok(ValidationResult {
        is_safe,
        category,
        warnings,
        errors,
    })
}

/// Validate file size against the maximum allowed.
pub fn validate_file_size(size: u64) -> Result<()> {
    if size > MAX_FILE_SIZE {
        return Err(Error::Other(format!(
            "File too large: {} bytes (max {} bytes)",
            size, MAX_FILE_SIZE
        )));
    }
    Ok(())
}

/// Compute a CRC32 checksum for file chunk verification.
pub fn compute_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Verify a CRC32 checksum.
pub fn verify_crc32(data: &[u8], expected: u32) -> bool {
    compute_crc32(data) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_png() {
        let data = b"\x89PNG\r\n\x1a\nrest of file";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Image);
        assert_eq!(name, Some("PNG"));
    }

    #[test]
    fn test_detect_jpeg() {
        let data = b"\xFF\xD8\xFF\xE0";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Image);
        assert_eq!(name, Some("JPEG"));
    }

    #[test]
    fn test_detect_zip() {
        let data = b"PK\x03\x04contents";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Archive);
        assert_eq!(name, Some("ZIP"));
    }

    #[test]
    fn test_detect_exe() {
        let data = b"MZ\x90\x00";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Executable);
        assert_eq!(name, Some("EXE/PE"));
    }

    #[test]
    fn test_detect_mp4() {
        let data = b"\x00\x00\x00\x1c\x66\x74\x79\x70isom";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Video);
        assert_eq!(name, Some("MP4"));
    }

    #[test]
    fn test_detect_pdf() {
        let data = b"%PDF-1.4 rest";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Document);
        assert_eq!(name, Some("PDF"));
    }

    #[test]
    fn test_detect_unknown() {
        let data = b"random data here";
        let (cat, name) = detect_file_type(data);
        assert_eq!(cat, FileCategory::Unknown);
        assert!(name.is_none());
    }

    #[test]
    fn test_detect_empty() {
        let (cat, name) = detect_file_type(&[]);
        assert_eq!(cat, FileCategory::Unknown);
        assert!(name.is_none());
    }

    #[test]
    fn test_validate_filename_safe() {
        let result = validate_filename("document.pdf").unwrap();
        assert!(result.is_safe);
    }

    #[test]
    fn test_validate_filename_exe_blocked() {
        let result = validate_filename("malware.exe").unwrap();
        assert!(!result.is_safe);
        assert!(result.errors.iter().any(|e| e.contains("Blocked")));
    }

    #[test]
    fn test_validate_filename_path_traversal() {
        let result = validate_filename("../../etc/passwd").unwrap();
        assert!(!result.is_safe);
        assert!(result.errors.iter().any(|e| e.contains("traversal")));
    }

    #[test]
    fn test_validate_filename_double_extension() {
        let result = validate_filename("document.txt.exe").unwrap();
        assert!(!result.is_safe);
        assert!(result.errors.iter().any(|e| e.contains("double-extension")));
    }

    #[test]
    fn test_validate_filename_null_bytes() {
        let result = validate_filename("file\0.exe").unwrap();
        assert!(!result.is_safe);
    }

    #[test]
    fn test_validate_filename_too_long() {
        let long_name = "a".repeat(300);
        let result = validate_filename(&long_name).unwrap();
        assert!(!result.is_safe);
    }

    #[test]
    fn test_validate_filename_hidden_file_warning() {
        let result = validate_filename(".bashrc").unwrap();
        assert!(result.is_safe);
        assert!(result.warnings.iter().any(|w| w.contains("Hidden")));
    }

    #[test]
    fn test_validate_content_executable_rejected() {
        let data = b"MZ\x90\x00\x03\x00";
        let result = validate_file_content(data, "document.pdf").unwrap();
        assert!(!result.is_safe);
        assert_eq!(result.category, FileCategory::Executable);
    }

    #[test]
    fn test_validate_content_safe_image() {
        let data = b"\x89PNG\r\n\x1a\nimage data";
        let result = validate_file_content(data, "photo.png").unwrap();
        assert!(result.is_safe);
        assert_eq!(result.category, FileCategory::Image);
    }

    #[test]
    fn test_validate_content_extension_mismatch() {
        let data = b"\x89PNG\r\n\x1a\nimage data";
        let result = validate_file_content(data, "document.pdf").unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("doesn't match")));
    }

    #[test]
    fn test_validate_file_size_ok() {
        assert!(validate_file_size(1024).is_ok());
        assert!(validate_file_size(MAX_FILE_SIZE).is_ok());
    }

    #[test]
    fn test_validate_file_size_too_large() {
        assert!(validate_file_size(MAX_FILE_SIZE + 1).is_err());
    }

    #[test]
    fn test_crc32_known_vector() {
        // CRC32 of "123456789" = 0xCBF43926
        let crc = compute_crc32(b"123456789");
        assert_eq!(crc, 0xCBF43926);
    }

    #[test]
    fn test_crc32_verify() {
        let data = b"hello world";
        let crc = compute_crc32(data);
        assert!(verify_crc32(data, crc));
        assert!(!verify_crc32(data, crc ^ 0xFF));
    }

    #[test]
    fn test_crc32_empty() {
        let crc = compute_crc32(b"");
        assert_eq!(crc, 0x00000000);
    }

    #[test]
    fn test_validate_all_blocked_extensions() {
        for ext in BLOCKED_EXTENSIONS {
            let result = validate_filename(&format!("test.{}", ext)).unwrap();
            assert!(!result.is_safe, ".{} should be blocked", ext);
        }
    }

    #[test]
    fn test_validate_safe_extensions() {
        for ext in &["txt", "pdf", "png", "jpg", "mp4", "zip", "docx", "xlsx"] {
            let result = validate_filename(&format!("safe.{}", ext)).unwrap();
            assert!(result.is_safe, ".{} should be safe", ext);
        }
    }
}
