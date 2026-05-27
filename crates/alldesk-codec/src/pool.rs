//! Frame buffer memory pool for reducing allocations in the encode/decode pipeline.
//!
//! Pre-allocates fixed-size buffers and recycles them to avoid frequent heap allocations
//! during real-time video processing.

use std::collections::VecDeque;
use std::sync::Mutex;

/// Default buffer size for a 1920x1080 BGRA frame (approx 8 MB).
pub const DEFAULT_FRAME_BUFFER_SIZE: usize = 1920 * 1080 * 4;

/// Maximum number of buffers to keep in the pool.
const DEFAULT_POOL_CAPACITY: usize = 8;

/// A pool of reusable byte buffers for frame data.
pub struct BufferPool {
    /// Available buffers ready for reuse.
    buffers: Mutex<VecDeque<Vec<u8>>>,
    /// Size of each buffer in the pool.
    buffer_size: usize,
    /// Maximum number of buffers to retain.
    max_capacity: usize,
    /// Total number of buffers ever allocated (for stats).
    total_allocated: Mutex<u64>,
    /// Total number of buffers reused (for stats).
    total_reused: Mutex<u64>,
}

impl BufferPool {
    /// Create a new buffer pool with default settings.
    pub fn new() -> Self {
        Self::with_config(DEFAULT_FRAME_BUFFER_SIZE, DEFAULT_POOL_CAPACITY)
    }

    /// Create a new buffer pool with custom buffer size and capacity.
    pub fn with_config(buffer_size: usize, max_capacity: usize) -> Self {
        Self {
            buffers: Mutex::new(VecDeque::with_capacity(max_capacity)),
            buffer_size,
            max_capacity,
            total_allocated: Mutex::new(0),
            total_reused: Mutex::new(0),
        }
    }

    /// Acquire a buffer from the pool. Returns a reused buffer if available,
    /// or allocates a new one if the pool is empty.
    pub fn acquire(&self) -> Vec<u8> {
        let mut buffers = self.buffers.lock().unwrap();
        if let Some(mut buf) = buffers.pop_front() {
            buf.clear();
            *self.total_reused.lock().unwrap() += 1;
            buf
        } else {
            *self.total_allocated.lock().unwrap() += 1;
            Vec::with_capacity(self.buffer_size)
        }
    }

    /// Acquire a buffer with a specific minimum capacity.
    pub fn acquire_with_capacity(&self, min_capacity: usize) -> Vec<u8> {
        let mut buffers = self.buffers.lock().unwrap();
        if let Some(mut buf) = buffers.pop_front() {
            buf.clear();
            if buf.capacity() < min_capacity {
                buf.reserve(min_capacity);
            }
            *self.total_reused.lock().unwrap() += 1;
            buf
        } else {
            *self.total_allocated.lock().unwrap() += 1;
            let buf = Vec::with_capacity(min_capacity.max(self.buffer_size));
            buf
        }
    }

    /// Return a buffer to the pool for reuse. The buffer is dropped if the pool is full.
    pub fn release(&self, buf: Vec<u8>) {
        let mut buffers = self.buffers.lock().unwrap();
        if buffers.len() < self.max_capacity {
            buffers.push_back(buf);
        }
        // Otherwise, the buffer is dropped and its memory is freed
    }

    /// Number of buffers currently available in the pool.
    pub fn available(&self) -> usize {
        self.buffers.lock().unwrap().len()
    }

    /// Get pool statistics.
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            available: self.buffers.lock().unwrap().len(),
            buffer_size: self.buffer_size,
            max_capacity: self.max_capacity,
            total_allocated: *self.total_allocated.lock().unwrap(),
            total_reused: *self.total_reused.lock().unwrap(),
        }
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the buffer pool usage.
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub available: usize,
    pub buffer_size: usize,
    pub max_capacity: usize,
    pub total_allocated: u64,
    pub total_reused: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_new() {
        let pool = BufferPool::new();
        assert_eq!(pool.available(), 0);
        let stats = pool.stats();
        assert_eq!(stats.buffer_size, DEFAULT_FRAME_BUFFER_SIZE);
        assert_eq!(stats.max_capacity, DEFAULT_POOL_CAPACITY);
    }

    #[test]
    fn test_buffer_pool_acquire_release() {
        let pool = BufferPool::with_config(1024, 4);

        // Acquire first buffer (allocates)
        let buf = pool.acquire();
        assert_eq!(pool.available(), 0);
        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 1);
        assert_eq!(stats.total_reused, 0);

        // Release it
        pool.release(buf);
        assert_eq!(pool.available(), 1);

        // Acquire again (reuses)
        let buf = pool.acquire();
        assert_eq!(pool.available(), 0);
        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 1);
        assert_eq!(stats.total_reused, 1);

        pool.release(buf);
    }

    #[test]
    fn test_buffer_pool_max_capacity() {
        let pool = BufferPool::with_config(1024, 2);

        let b1 = pool.acquire();
        let b2 = pool.acquire();
        let b3 = pool.acquire();

        pool.release(b1);
        pool.release(b2);
        pool.release(b3); // This one should be dropped

        assert_eq!(pool.available(), 2);
    }

    #[test]
    fn test_buffer_pool_acquire_with_capacity() {
        let pool = BufferPool::with_config(64, 4);
        let buf = pool.acquire_with_capacity(256);
        assert!(buf.capacity() >= 256);
    }

    #[test]
    fn test_buffer_pool_reuse_clears() {
        let pool = BufferPool::with_config(1024, 4);

        let mut buf = pool.acquire();
        buf.extend_from_slice(b"hello");
        assert_eq!(buf.len(), 5);
        pool.release(buf);

        // Reused buffer should be cleared
        let buf = pool.acquire();
        assert_eq!(buf.len(), 0);
        pool.release(buf);
    }

    #[test]
    fn test_pool_stats() {
        let pool = BufferPool::with_config(1024, 4);
        let b1 = pool.acquire();
        let b2 = pool.acquire();
        pool.release(b1);

        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 2);
        assert_eq!(stats.available, 1);
        pool.release(b2);
    }
}
