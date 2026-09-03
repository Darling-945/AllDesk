use std::fs;
use std::path::Path;

use alldesk_core::Result;

/// Represents a file or directory entry in a scanned tree.
#[derive(Debug, Clone)]
pub struct FileManifest {
    /// File or directory name (not the full path, just the basename).
    pub name: String,
    /// Size in bytes. For directories this is the sum of all children.
    pub size: u64,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Child entries (only populated for directories).
    pub children: Vec<FileManifest>,
}

impl FileManifest {
    /// Scan a directory and build a recursive tree of FileManifest entries.
    ///
    /// Uses synchronous std::fs calls since directory listing is fast and
    /// non-blocking for typical directory sizes.
    pub fn scan_dir(path: &str) -> Result<Self> {
        let dir_path = Path::new(path);

        if !dir_path.exists() {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory not found: {}", path),
            )));
        }

        if !dir_path.is_dir() {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                format!("Path is not a directory: {}", path),
            )));
        }

        let name = dir_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        let mut children = Vec::new();
        let mut total_size: u64 = 0;

        let entries = match fs::read_dir(dir_path) {
            Ok(entries) => entries,
            Err(e) => {
                // If we can't read the directory, return what we have.
                tracing::warn!("Cannot read directory {}: {}", path, e);
                return Ok(Self {
                    name,
                    size: 0,
                    is_dir: true,
                    children: Vec::new(),
                });
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Skipping unreadable entry: {}", e);
                    continue;
                }
            };

            let entry_path = entry.path();
            let entry_name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let metadata = match entry_path.symlink_metadata() {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Cannot read metadata for {:?}: {}", entry_path, e);
                    continue;
                }
            };

            // Skip symbolic links to avoid cycles.
            if metadata.file_type().is_symlink() {
                continue;
            }

            if metadata.is_dir() {
                let child_manifest = match Self::scan_dir(&entry_path.to_string_lossy()) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Cannot scan subdir {:?}: {}", entry_path, e);
                        continue;
                    }
                };
                total_size += child_manifest.size;
                children.push(child_manifest);
            } else {
                let file_size = metadata.len();
                total_size += file_size;
                children.push(FileManifest {
                    name: entry_name,
                    size: file_size,
                    is_dir: false,
                    children: Vec::new(),
                });
            }
        }

        // Sort children: directories first, then alphabetically.
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(Self {
            name,
            size: total_size,
            is_dir: true,
            children,
        })
    }

    /// Count total number of files (non-directory entries) in this tree.
    pub fn file_count(&self) -> u64 {
        if self.is_dir {
            self.children.iter().map(|c| c.file_count()).sum()
        } else {
            1
        }
    }

    /// Count total number of directories in this tree (excluding self).
    pub fn dir_count(&self) -> u64 {
        if self.is_dir {
            let self_count: u64 = 1;
            let child_count: u64 = self.children.iter().map(|c| c.dir_count()).sum();
            self_count + child_count
        } else {
            0
        }
    }

    /// Returns true if this manifest has no children (empty directory or empty file).
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("alldesk_test_manifest")
            .join(name);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_scan_dir_basic() {
        let dir = temp_dir("basic");
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.join("b.txt"), b"world").unwrap();
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join("subdir").join("c.txt"), b"sub").unwrap();

        let manifest = FileManifest::scan_dir(&dir.to_string_lossy()).unwrap();

        assert!(manifest.is_dir);
        assert_eq!(manifest.children.len(), 3); // a.txt, b.txt, subdir

        // Directories should be sorted first
        assert!(manifest.children[0].is_dir); // subdir
        assert!(!manifest.children[1].is_dir); // a.txt
        assert!(!manifest.children[2].is_dir); // b.txt

        // Size should include subdirectory content
        assert!(manifest.size >= 11); // "hello" + "world" + "sub" at minimum

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_dir_nonexistent() {
        let result = FileManifest::scan_dir("/nonexistent/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_dir_empty() {
        let dir = temp_dir("empty_dir");
        let _ = std::fs::create_dir_all(&dir);

        let manifest = FileManifest::scan_dir(&dir.to_string_lossy()).unwrap();
        assert!(manifest.is_dir);
        assert!(manifest.is_empty());
        assert_eq!(manifest.size, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_counts() {
        let dir = temp_dir("counts");
        std::fs::write(dir.join("a.txt"), b"a").unwrap();
        std::fs::write(dir.join("b.txt"), b"b").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("c.txt"), b"c").unwrap();

        let manifest = FileManifest::scan_dir(&dir.to_string_lossy()).unwrap();
        assert_eq!(manifest.file_count(), 3); // a.txt, b.txt, c.txt
        assert_eq!(manifest.dir_count(), 2); // root + sub

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_dir_not_a_directory() {
        let dir = temp_dir("not_a_dir");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("file.txt");
        std::fs::write(&file, b"not a dir").unwrap();

        let result = FileManifest::scan_dir(&file.to_string_lossy());
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
