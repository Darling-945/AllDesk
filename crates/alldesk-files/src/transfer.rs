use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use alldesk_core::Result;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

/// Size of each chunk when reading/writing files.
pub const CHUNK_SIZE: usize = 64 * 1024; // 64 KB

/// Fixed-point scaling for progress: we store progress as u64 where
/// 1_000_000 = 1.0 (i.e., six decimal places of precision).
const PROGRESS_SCALE: u64 = 1_000_000;

/// A single chunk of a file being transferred.
#[derive(Debug, Clone)]
pub struct FileChunk {
    /// Index of this chunk (0-based).
    pub index: u64,
    /// Raw byte data of this chunk.
    pub data: Vec<u8>,
    /// Whether this is the last chunk.
    pub is_last: bool,
    /// Optional CRC32 checksum for integrity verification.
    pub checksum: Option<u32>,
}

impl FileChunk {
    /// Compute a CRC32 checksum of the chunk data.
    pub fn compute_checksum(&self) -> u32 {
        crc32(&self.data)
    }

    /// Verify the chunk checksum. Returns true if valid or no checksum set.
    pub fn verify_checksum(&self) -> bool {
        match self.checksum {
            Some(expected) => crc32(&self.data) == expected,
            None => true,
        }
    }
}

/// Simple CRC32 implementation for chunk integrity.
fn crc32(data: &[u8]) -> u32 {
    const CRC32_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0u32;
        while i < 256 {
            let mut crc = i;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i as usize] = crc;
            i += 1;
        }
        table
    };

    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        let index = ((crc ^ (*byte as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

/// Callback type for progress reporting.
/// Arguments: (bytes_transferred, total_bytes, fraction 0.0-1.0)
pub type ProgressCallback = dyn Fn(u64, u64, f64) + Send + Sync;

/// Handles sending and receiving files in chunks with progress tracking.
pub struct FileTransfer {
    bytes_transferred: Arc<AtomicU64>,
    total_bytes: Arc<AtomicU64>,
    /// Progress stored as fixed-point u64: value / PROGRESS_SCALE = fraction.
    progress_fixed: Arc<AtomicU64>,
    progress_callback: Option<Arc<ProgressCallback>>,
}

/// Convert a fraction (0.0-1.0) to a fixed-point u64.
#[inline]
fn frac_to_fixed(frac: f64) -> u64 {
    (frac * PROGRESS_SCALE as f64) as u64
}

/// Convert a fixed-point u64 back to a fraction.
#[inline]
fn fixed_to_frac(fixed: u64) -> f64 {
    fixed as f64 / PROGRESS_SCALE as f64
}

impl FileTransfer {
    /// Create a new FileTransfer instance.
    pub fn new() -> Self {
        Self {
            bytes_transferred: Arc::new(AtomicU64::new(0)),
            total_bytes: Arc::new(AtomicU64::new(0)),
            progress_fixed: Arc::new(AtomicU64::new(0)),
            progress_callback: None,
        }
    }

    /// Create a new FileTransfer with a progress callback.
    pub fn with_progress_callback(callback: Arc<ProgressCallback>) -> Self {
        Self {
            bytes_transferred: Arc::new(AtomicU64::new(0)),
            total_bytes: Arc::new(AtomicU64::new(0)),
            progress_fixed: Arc::new(AtomicU64::new(0)),
            progress_callback: Some(callback),
        }
    }

    /// Returns current transfer progress as a fraction (0.0 - 1.0).
    pub fn progress(&self) -> f64 {
        fixed_to_frac(self.progress_fixed.load(Ordering::Relaxed))
    }

    /// Returns the number of bytes transferred so far.
    pub fn bytes_transferred(&self) -> u64 {
        self.bytes_transferred.load(Ordering::Relaxed)
    }

    /// Returns the total size in bytes of the current transfer.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Read a file from disk in 64 KB chunks, writing each chunk directly to a
    /// destination writer. This avoids loading the entire file into memory.
    ///
    /// The `write_fn` callback is invoked for each chunk, allowing the caller
    /// to send chunks over the network without buffering them all.
    pub async fn send_file<F, Fut>(&self, path: &str, mut write_fn: F) -> Result<()>
    where
        F: FnMut(&FileChunk) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let file_path = Path::new(path);

        // Validate the file exists and is readable.
        if !file_path.exists() {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path),
            )));
        }

        let metadata = tokio::fs::metadata(path).await?;
        let file_size = metadata.len();
        self.total_bytes.store(file_size, Ordering::Relaxed);
        self.bytes_transferred.store(0, Ordering::Relaxed);
        self.progress_fixed.store(0, Ordering::Relaxed);

        let file = File::open(path).await?;
        let mut reader = BufReader::new(file);

        let mut index: u64 = 0;
        let mut bytes_read_total: u64 = 0;

        loop {
            let mut buffer = vec![0u8; CHUNK_SIZE];
            let bytes_read = reader.read(&mut buffer).await?;

            if bytes_read == 0 {
                // We've reached EOF.
                break;
            }

            buffer.truncate(bytes_read);
            bytes_read_total += bytes_read as u64;

            let is_last = bytes_read_total >= file_size;

            let chunk = FileChunk {
                index,
                data: buffer,
                is_last,
                checksum: None,
            };

            write_fn(&chunk).await?;

            // Update progress.
            self.bytes_transferred.store(bytes_read_total, Ordering::Relaxed);
            let frac = if file_size > 0 {
                bytes_read_total as f64 / file_size as f64
            } else {
                1.0
            };
            self.progress_fixed.store(frac_to_fixed(frac), Ordering::Relaxed);

            if let Some(ref cb) = self.progress_callback {
                cb(bytes_read_total, file_size, frac);
            }

            index += 1;

            if is_last {
                break;
            }
        }

        // Handle empty files: produce a single empty last chunk.
        if index == 0 {
            write_fn(&FileChunk {
                index: 0,
                data: Vec::new(),
                is_last: true,
                checksum: Some(0),
            }).await?;
            self.progress_fixed.store(frac_to_fixed(1.0), Ordering::Relaxed);
        }

        Ok(())
    }

    /// Write a sequence of chunks to a destination file on disk.
    ///
    /// Chunks are written in order. After all chunks are written, the file
    /// is flushed and sync'd to ensure data integrity.
    pub async fn receive_file(&self, chunks: &[FileChunk], dest: &str) -> Result<()> {
        let dest_path = PathBuf::from(dest);

        // Ensure parent directory exists.
        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let total_data_size: u64 = chunks.iter().map(|c| c.data.len() as u64).sum();
        self.total_bytes.store(total_data_size, Ordering::Relaxed);
        self.bytes_transferred.store(0, Ordering::Relaxed);
        self.progress_fixed.store(0, Ordering::Relaxed);

        let file = File::create(&dest_path).await?;
        let mut writer = BufWriter::new(file);

        let mut bytes_written: u64 = 0;

        for chunk in chunks {
            writer.write_all(&chunk.data).await?;
            bytes_written += chunk.data.len() as u64;

            // Update progress.
            self.bytes_transferred.store(bytes_written, Ordering::Relaxed);
            let frac = if total_data_size > 0 {
                bytes_written as f64 / total_data_size as f64
            } else {
                1.0
            };
            self.progress_fixed.store(frac_to_fixed(frac), Ordering::Relaxed);

            if let Some(ref cb) = self.progress_callback {
                cb(bytes_written, total_data_size, frac);
            }
        }

        writer.flush().await?;

        // Sync to disk for integrity.
        {
            let file = writer.into_inner();
            file.sync_all().await?;
        }

        Ok(())
    }

    /// Convenience: read a file chunk-by-chunk and write each chunk to dest.
    /// Useful for local file copy with progress tracking.
    /// Only one chunk is held in memory at a time.
    pub async fn copy_file(&self, src: &str, dest: &str) -> Result<()> {
        let src_path = Path::new(src);
        if !src_path.exists() {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", src),
            )));
        }

        let dest_path = PathBuf::from(dest);
        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let metadata = tokio::fs::metadata(src).await?;
        let file_size = metadata.len();
        self.total_bytes.store(file_size, Ordering::Relaxed);
        self.bytes_transferred.store(0, Ordering::Relaxed);
        self.progress_fixed.store(0, Ordering::Relaxed);

        let src_file = File::open(src).await?;
        let mut reader = BufReader::new(src_file);

        let dest_file = File::create(&dest_path).await?;
        let mut writer = BufWriter::new(dest_file);

        let mut bytes_copied: u64 = 0;

        loop {
            let mut buffer = vec![0u8; CHUNK_SIZE];
            let bytes_read = reader.read(&mut buffer).await?;

            if bytes_read == 0 {
                break;
            }

            writer.write_all(&buffer[..bytes_read]).await?;
            bytes_copied += bytes_read as u64;

            // Update progress.
            self.bytes_transferred.store(bytes_copied, Ordering::Relaxed);
            let frac = if file_size > 0 {
                bytes_copied as f64 / file_size as f64
            } else {
                1.0
            };
            self.progress_fixed.store(frac_to_fixed(frac), Ordering::Relaxed);

            if let Some(ref cb) = self.progress_callback {
                cb(bytes_copied, file_size, frac);
            }
        }

        writer.flush().await?;
        {
            let file = writer.into_inner();
            file.sync_all().await?;
        }

        Ok(())
    }
}

impl Default for FileTransfer {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent manifest for tracking chunk transfer state, enabling resume.
///
/// Stored as a JSON sidecar file (e.g., `dest.part`) alongside the partial download.
/// Each chunk's received state is tracked so transfer can resume after interruption.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferManifest {
    /// Source file path (or identifier).
    pub source: String,
    /// Destination file path.
    pub destination: String,
    /// Total file size in bytes.
    pub total_size: u64,
    /// Total number of chunks.
    pub total_chunks: u64,
    /// Chunk size in bytes.
    pub chunk_size: u64,
    /// Set of completed chunk indices.
    pub completed_chunks: std::collections::HashSet<u64>,
    /// Optional CRC32 checksums per chunk index.
    pub checksums: std::collections::HashMap<u64, u32>,
}

impl TransferManifest {
    /// Create a new manifest for a file transfer.
    pub fn new(source: &str, destination: &str, total_size: u64, chunk_size: u64) -> Self {
        let total_chunks = if total_size == 0 {
            1
        } else {
            total_size.div_ceil(chunk_size)
        };
        Self {
            source: source.to_string(),
            destination: destination.to_string(),
            total_size,
            total_chunks,
            chunk_size,
            completed_chunks: std::collections::HashSet::new(),
            checksums: std::collections::HashMap::new(),
        }
    }

    /// Mark a chunk as completed.
    pub fn mark_chunk(&mut self, index: u64) {
        self.completed_chunks.insert(index);
    }

    /// Check if a chunk has been completed.
    pub fn is_chunk_done(&self, index: u64) -> bool {
        self.completed_chunks.contains(&index)
    }

    /// Returns the fraction of completed chunks (0.0 - 1.0).
    pub fn progress(&self) -> f64 {
        if self.total_chunks == 0 {
            return 1.0;
        }
        self.completed_chunks.len() as f64 / self.total_chunks as f64
    }

    /// Returns true if all chunks are completed.
    pub fn is_complete(&self) -> bool {
        self.completed_chunks.len() as u64 == self.total_chunks
    }

    /// Returns the list of chunk indices not yet received.
    pub fn missing_chunks(&self) -> Vec<u64> {
        (0..self.total_chunks)
            .filter(|i| !self.completed_chunks.contains(i))
            .collect()
    }

    /// Save the manifest to a JSON sidecar file next to the destination.
    pub async fn save(&self) -> Result<()> {
        let path = format!("{}.part", self.destination);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| alldesk_core::Error::Io(std::io::Error::other(
                format!("serialize manifest: {}", e),
            )))?;
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    /// Load a manifest from a sidecar file.
    pub async fn load(destination: &str) -> Result<Self> {
        let path = format!("{}.part", destination);
        let data = tokio::fs::read_to_string(&path).await?;
        let manifest: TransferManifest = serde_json::from_str(&data)
            .map_err(|e| alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("parse manifest: {}", e),
            )))?;
        Ok(manifest)
    }

    /// Delete the manifest sidecar file after successful transfer.
    pub async fn cleanup(&self) -> Result<()> {
        let path = format!("{}.part", self.destination);
        let _ = tokio::fs::remove_file(&path).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("alldesk_test_files").join(name);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn test_file_transfer_send_and_receive() {
        let dir = temp_dir("send_recv");
        let src = dir.join("source.bin");
        let dest = dir.join("dest.bin");

        // Create a test file
        let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let ft = FileTransfer::new();
        let mut chunks = Vec::new();

        ft.send_file(&src.to_string_lossy(), |chunk| {
            chunks.push(FileChunk {
                index: chunk.index,
                data: chunk.data.clone(),
                is_last: chunk.is_last,
                checksum: chunk.checksum,
            });
            std::future::ready(Ok(()))
        }).await.unwrap();

        assert!(!chunks.is_empty());
        assert!(chunks.last().unwrap().is_last);

        ft.receive_file(&chunks, &dest.to_string_lossy()).await.unwrap();

        let result = std::fs::read(&dest).unwrap();
        assert_eq!(result, data);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_file_transfer_empty_file() {
        let dir = temp_dir("empty");
        let src = dir.join("empty.bin");
        let _dest = dir.join("empty_dest.bin");

        std::fs::write(&src, b"").unwrap();

        let ft = FileTransfer::new();
        let mut chunks = Vec::new();

        ft.send_file(&src.to_string_lossy(), |chunk| {
            chunks.push(FileChunk {
                index: chunk.index,
                data: chunk.data.clone(),
                is_last: chunk.is_last,
                checksum: chunk.checksum,
            });
            std::future::ready(Ok(()))
        }).await.unwrap();

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_last);
        assert!(chunks[0].data.is_empty());

        ft.receive_file(&chunks, &_dest.to_string_lossy()).await.unwrap();
        let result = std::fs::read(&_dest).unwrap();
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_file_transfer_missing_source() {
        let ft = FileTransfer::new();
        let result = ft.send_file("/nonexistent/file.bin", |_| std::future::ready(Ok(()))).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_transfer_progress_tracking() {
        let dir = temp_dir("progress");
        let src = dir.join("source.bin");
        let _dest = dir.join("dest.bin");

        let data = vec![0xABu8; 128 * 1024]; // 128 KB
        std::fs::write(&src, &data).unwrap();

        let ft = FileTransfer::new();
        let mut chunks = Vec::new();

        ft.send_file(&src.to_string_lossy(), |chunk| {
            chunks.push(FileChunk {
                index: chunk.index,
                data: chunk.data.clone(),
                is_last: chunk.is_last,
                checksum: chunk.checksum,
            });
            std::future::ready(Ok(()))
        }).await.unwrap();

        // Progress should be 1.0 after completion
        let progress = ft.progress();
        assert!((progress - 1.0).abs() < 0.01);
        assert_eq!(ft.bytes_transferred(), data.len() as u64);
        assert_eq!(ft.total_bytes(), data.len() as u64);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_file_copy() {
        let dir = temp_dir("copy");
        let src = dir.join("original.bin");
        let dest = dir.join("copy.bin");

        let data: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let ft = FileTransfer::new();
        ft.copy_file(&src.to_string_lossy(), &dest.to_string_lossy()).await.unwrap();

        let result = std::fs::read(&dest).unwrap();
        assert_eq!(result, data);
        assert!((ft.progress() - 1.0).abs() < 0.01);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fixed_point_conversion() {
        assert_eq!(frac_to_fixed(0.0), 0);
        assert_eq!(frac_to_fixed(1.0), 1_000_000);
        assert!((fixed_to_frac(frac_to_fixed(0.5)) - 0.5).abs() < 0.0001);
    }

    #[test]
    fn test_crc32_deterministic() {
        let data = b"hello world";
        let a = crc32(data);
        let b = crc32(data);
        assert_eq!(a, b);
    }

    #[test]
    fn test_crc32_different_data() {
        let a = crc32(b"hello");
        let b = crc32(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_chunk_checksum_compute_and_verify() {
        let chunk = FileChunk {
            index: 0,
            data: b"test data".to_vec(),
            is_last: true,
            checksum: None,
        };

        let checksum = chunk.compute_checksum();
        assert_ne!(checksum, 0);

        let verified_chunk = FileChunk {
            index: 0,
            data: b"test data".to_vec(),
            is_last: true,
            checksum: Some(checksum),
        };
        assert!(verified_chunk.verify_checksum());

        // Corrupted data should fail verification
        let corrupted = FileChunk {
            index: 0,
            data: b"test dat!".to_vec(),
            is_last: true,
            checksum: Some(checksum),
        };
        assert!(!corrupted.verify_checksum());
    }

    #[test]
    fn test_chunk_checksum_none_always_valid() {
        let chunk = FileChunk {
            index: 0,
            data: b"any data".to_vec(),
            is_last: false,
            checksum: None,
        };
        assert!(chunk.verify_checksum());
    }

    #[test]
    fn test_crc32_empty_data() {
        let checksum = crc32(b"");
        assert_eq!(checksum, 0x00000000);
    }

    #[test]
    fn test_manifest_new() {
        let m = TransferManifest::new("/src/file.bin", "/dest/file.bin", 1024 * 1024, 64 * 1024);
        assert_eq!(m.total_size, 1024 * 1024);
        assert_eq!(m.total_chunks, 16); // 1MB / 64KB
        assert!(m.completed_chunks.is_empty());
        assert!(m.missing_chunks().len() == 16);
        assert!(!m.is_complete());
        assert!((m.progress() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_manifest_progress_and_completion() {
        let mut m = TransferManifest::new("src", "dst", 256 * 1024, 64 * 1024);
        assert_eq!(m.total_chunks, 4);

        m.mark_chunk(0);
        assert!(m.is_chunk_done(0));
        assert!(!m.is_chunk_done(1));
        assert!((m.progress() - 0.25).abs() < f64::EPSILON);
        assert_eq!(m.missing_chunks(), vec![1, 2, 3]);

        m.mark_chunk(1);
        m.mark_chunk(2);
        m.mark_chunk(3);
        assert!(m.is_complete());
        assert!(m.missing_chunks().is_empty());
    }

    #[test]
    fn test_manifest_save_load_roundtrip() {
        let dir = temp_dir("manifest");
        let dest = dir.join("file.bin");
        let dest_str = dest.to_string_lossy().to_string();

        let mut m = TransferManifest::new("src.bin", &dest_str, 128 * 1024, 64 * 1024);
        m.mark_chunk(0);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            m.save().await.unwrap();
            let loaded = TransferManifest::load(&dest_str).await.unwrap();
            assert_eq!(loaded.source, "src.bin");
            assert_eq!(loaded.total_chunks, 2);
            assert!(loaded.is_chunk_done(0));
            assert!(!loaded.is_chunk_done(1));
        });

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_manifest_empty_file() {
        let m = TransferManifest::new("src", "dst", 0, 64 * 1024);
        assert_eq!(m.total_chunks, 1);
    }
}
