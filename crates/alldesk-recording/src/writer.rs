use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use alldesk_core::Result;

/// Magic bytes identifying an ALDREC v1 recording file (video only).
const MAGIC_V1: &[u8; 6] = b"ALDREC";

/// Magic bytes identifying an ALDREC v2 recording file (video + audio).
const MAGIC_V2: &[u8; 6] = b"ALDRC2";

/// v1 header size: magic(6) + width(4) + height(4) + fps(4) + frame_count(4) = 22 bytes.
const HEADER_V1_SIZE: usize = 22;

/// v2 header size: magic(6) + width(4) + height(4) + fps(4) + video_frame_count(4)
///                + audio_sample_rate(4) + audio_frame_count(4) = 30 bytes.
const HEADER_V2_SIZE: usize = 30;

/// Offsets within the header for each field (v1 and v2 share the same layout for first 22 bytes).
const OFFSET_WIDTH: u64 = 6;
const OFFSET_HEIGHT: u64 = 10;
const OFFSET_FPS: u64 = 14;
const OFFSET_FRAME_COUNT: u64 = 18;
const OFFSET_AUDIO_SAMPLE_RATE: u64 = 22;
const OFFSET_AUDIO_FRAME_COUNT: u64 = 26;

/// A single audio frame from a recording.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Presentation timestamp in milliseconds relative to recording start.
    pub timestamp_ms: u64,
    /// Raw f32 LE audio samples (48kHz mono).
    pub samples: Vec<u8>,
}

/// Writes screen recording frames to a custom container format.
///
/// Supports two versions:
/// - v1 (ALDREC): video only, backward compatible.
/// - v2 (ALDRC2): video + audio tracks.
///
/// File format (v2):
/// ```text
/// [Header]
///   magic: "ALDRC2" (6 bytes)
///   width: u32 LE (4 bytes)
///   height: u32 LE (4 bytes)
///   fps: u32 LE (4 bytes)
///   video_frame_count: u32 LE (4 bytes) -- written on finish()
///   audio_sample_rate: u32 LE (4 bytes) -- 0 if no audio
///   audio_frame_count: u32 LE (4 bytes) -- written on finish()
///
/// [Video Frame * N]
///   timestamp_ms: u64 LE (8 bytes)
///   data_len: u32 LE (4 bytes)
///   pixel_data: [u8; data_len]
///
/// [Audio Frame * M] (only if audio_sample_rate > 0)
///   timestamp_ms: u64 LE (8 bytes)
///   data_len: u32 LE (4 bytes)
///   sample_data: [u8; data_len] (raw f32 LE samples)
/// ```
pub struct Recorder {
    file: std::fs::File,
    width: u32,
    height: u32,
    fps: u32,
    frame_count: u32,
    audio_sample_rate: u32,
    audio_frame_count: u32,
    has_audio: bool,
}

impl Recorder {
    /// Create a new recording file at the given path (v2 format with audio support).
    pub fn new(output_path: &str) -> Result<Self> {
        let path = Path::new(output_path);

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut file = std::fs::File::create(path)?;

        // Write v2 header with defaults.
        let mut header = [0u8; HEADER_V2_SIZE];
        header[0..6].copy_from_slice(MAGIC_V2);

        file.write_all(&header)?;
        file.flush()?;

        Ok(Self {
            file,
            width: 0,
            height: 0,
            fps: 30,
            frame_count: 0,
            audio_sample_rate: 0,
            audio_frame_count: 0,
            has_audio: false,
        })
    }

    /// Set the video dimensions. Must be called before the first `write_frame`.
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        let _ = self.file.seek(SeekFrom::Start(OFFSET_WIDTH));
        let _ = self.file.write_all(&width.to_le_bytes());
        let _ = self.file.write_all(&height.to_le_bytes());
        let _ = self.file.seek(SeekFrom::End(0));
        self
    }

    /// Set the frame rate. Must be called before the first `write_frame`.
    pub fn with_fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        let _ = self.file.seek(SeekFrom::Start(OFFSET_FPS));
        let _ = self.file.write_all(&fps.to_le_bytes());
        let _ = self.file.seek(SeekFrom::End(0));
        self
    }

    /// Enable audio recording with the given sample rate (e.g., 48000).
    /// Must be called before writing any audio frames.
    pub fn with_audio(mut self, sample_rate: u32) -> Self {
        self.audio_sample_rate = sample_rate;
        self.has_audio = true;
        let _ = self.file.seek(SeekFrom::Start(OFFSET_AUDIO_SAMPLE_RATE));
        let _ = self.file.write_all(&sample_rate.to_le_bytes());
        let _ = self.file.seek(SeekFrom::End(0));
        self
    }

    /// Append a single video frame to the recording.
    pub fn write_frame(&mut self, data: &[u8], timestamp_ms: u64) -> Result<()> {
        self.file.write_all(&timestamp_ms.to_le_bytes())?;
        self.file.write_all(&(data.len() as u32).to_le_bytes())?;
        self.file.write_all(data)?;

        self.frame_count += 1;

        if self.frame_count % 30 == 0 {
            self.file.flush()?;
        }

        Ok(())
    }

    /// Append a single audio frame to the recording.
    /// `samples` is raw f32 LE audio data. `timestamp_ms` is the presentation timestamp.
    pub fn write_audio_frame(&mut self, samples: &[u8], timestamp_ms: u64) -> Result<()> {
        self.file.write_all(&timestamp_ms.to_le_bytes())?;
        self.file.write_all(&(samples.len() as u32).to_le_bytes())?;
        self.file.write_all(samples)?;

        self.audio_frame_count += 1;

        if self.audio_frame_count % 100 == 0 {
            self.file.flush()?;
        }

        Ok(())
    }

    /// Finalize the recording: write frame counts into the header, flush, and close.
    pub fn finish(mut self) -> Result<()> {
        // Write video frame count.
        self.file.seek(SeekFrom::Start(OFFSET_FRAME_COUNT))?;
        self.file.write_all(&self.frame_count.to_le_bytes())?;

        // Write audio frame count.
        self.file.seek(SeekFrom::Start(OFFSET_AUDIO_FRAME_COUNT))?;
        self.file.write_all(&self.audio_frame_count.to_le_bytes())?;

        self.file.flush()?;
        Ok(())
    }

    /// Returns the number of video frames written so far.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Returns the number of audio frames written so far.
    pub fn audio_frame_count(&self) -> u32 {
        self.audio_frame_count
    }

    /// Returns the configured video width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the configured video height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the audio sample rate (0 if no audio).
    pub fn audio_sample_rate(&self) -> u32 {
        self.audio_sample_rate
    }
}

/// Reads an ALDREC/ALDRC2 recording file and provides frame-by-frame access.
pub struct RecordingReader {
    data: Vec<u8>,
    width: u32,
    height: u32,
    fps: u32,
    frame_count: u32,
    audio_sample_rate: u32,
    audio_frame_count: u32,
    /// Offset where audio frames start (after all video frames).
    audio_offset: usize,
    /// Whether this is a v2 file.
    is_v2: bool,
}

impl RecordingReader {
    /// Open and parse an ALDREC/ALDRC2 recording file.
    pub fn open(path: &str) -> Result<Self> {
        let data = std::fs::read(path)?;

        if data.len() < HEADER_V1_SIZE {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "File too short to be a recording file",
            )));
        }

        let magic = &data[0..6];

        if magic == MAGIC_V2 {
            Self::read_v2(data)
        } else if magic == MAGIC_V1 {
            Self::read_v1(data)
        } else {
            Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Not a valid recording file (bad magic)",
            )))
        }
    }

    fn read_v1(data: Vec<u8>) -> Result<Self> {
        let width = u32::from_le_bytes(data[OFFSET_WIDTH as usize..(OFFSET_WIDTH + 4) as usize].try_into().unwrap());
        let height = u32::from_le_bytes(data[OFFSET_HEIGHT as usize..(OFFSET_HEIGHT + 4) as usize].try_into().unwrap());
        let fps = u32::from_le_bytes(data[OFFSET_FPS as usize..(OFFSET_FPS + 4) as usize].try_into().unwrap());
        let frame_count = u32::from_le_bytes(data[OFFSET_FRAME_COUNT as usize..(OFFSET_FRAME_COUNT + 4) as usize].try_into().unwrap());

        Ok(Self {
            data,
            width,
            height,
            fps,
            frame_count,
            audio_sample_rate: 0,
            audio_frame_count: 0,
            audio_offset: 0,
            is_v2: false,
        })
    }

    fn read_v2(data: Vec<u8>) -> Result<Self> {
        if data.len() < HEADER_V2_SIZE {
            return Err(alldesk_core::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "v2 file too short for header",
            )));
        }

        let width = u32::from_le_bytes(data[OFFSET_WIDTH as usize..(OFFSET_WIDTH + 4) as usize].try_into().unwrap());
        let height = u32::from_le_bytes(data[OFFSET_HEIGHT as usize..(OFFSET_HEIGHT + 4) as usize].try_into().unwrap());
        let fps = u32::from_le_bytes(data[OFFSET_FPS as usize..(OFFSET_FPS + 4) as usize].try_into().unwrap());
        let frame_count = u32::from_le_bytes(data[OFFSET_FRAME_COUNT as usize..(OFFSET_FRAME_COUNT + 4) as usize].try_into().unwrap());
        let audio_sample_rate = u32::from_le_bytes(data[OFFSET_AUDIO_SAMPLE_RATE as usize..(OFFSET_AUDIO_SAMPLE_RATE + 4) as usize].try_into().unwrap());
        let audio_frame_count = u32::from_le_bytes(data[OFFSET_AUDIO_FRAME_COUNT as usize..(OFFSET_AUDIO_FRAME_COUNT + 4) as usize].try_into().unwrap());

        // Find the audio offset by scanning through all video frames.
        let audio_offset = Self::find_audio_offset(&data, frame_count as usize);

        Ok(Self {
            data,
            width,
            height,
            fps,
            frame_count,
            audio_sample_rate,
            audio_frame_count,
            audio_offset,
            is_v2: true,
        })
    }

    /// Scan through video frames to find where audio data starts.
    fn find_audio_offset(data: &[u8], video_frame_count: usize) -> usize {
        let mut offset = HEADER_V2_SIZE;
        for _ in 0..video_frame_count {
            if offset + 12 > data.len() {
                break;
            }
            let data_len = u32::from_le_bytes(data[offset + 8..offset + 12].try_into().unwrap_or([0; 4])) as usize;
            offset += 12 + data_len;
        }
        offset
    }

    /// Returns the video width stored in the header.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the video height stored in the header.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the frame rate stored in the header.
    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Returns the video frame count stored in the header.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Returns the audio sample rate (0 if no audio).
    pub fn audio_sample_rate(&self) -> u32 {
        self.audio_sample_rate
    }

    /// Returns the audio frame count.
    pub fn audio_frame_count(&self) -> u32 {
        self.audio_frame_count
    }

    /// Iterate over all video frames.
    pub fn frames(&self) -> RecordingFrameIter<'_> {
        let start = if self.is_v2 { HEADER_V2_SIZE } else { HEADER_V1_SIZE };
        let end = if self.audio_offset > start {
            self.audio_offset
        } else {
            self.data.len()
        };
        RecordingFrameIter {
            data: &self.data,
            offset: start,
            end,
        }
    }

    /// Iterate over all audio frames.
    pub fn audio_frames(&self) -> AudioFrameIter<'_> {
        if self.audio_sample_rate == 0 || self.audio_offset >= self.data.len() {
            return AudioFrameIter {
                data: &self.data,
                offset: self.data.len(),
                end: self.data.len(),
            };
        }
        AudioFrameIter {
            data: &self.data,
            offset: self.audio_offset,
            end: self.data.len(),
        }
    }
}

/// Iterator over video frames in a recording.
pub struct RecordingFrameIter<'a> {
    data: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> Iterator for RecordingFrameIter<'a> {
    type Item = (u64, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + 12 > self.end {
            return None;
        }

        let timestamp_ms = u64::from_le_bytes(self.data[self.offset..self.offset + 8].try_into().ok()?);
        let data_len = u32::from_le_bytes(self.data[self.offset + 8..self.offset + 12].try_into().ok()?) as usize;

        let pixel_start = self.offset + 12;
        let pixel_end = pixel_start + data_len;

        if pixel_end > self.end {
            return None;
        }

        let pixel_data = &self.data[pixel_start..pixel_end];
        self.offset = pixel_end;

        Some((timestamp_ms, pixel_data))
    }
}

/// Iterator over audio frames in a recording.
pub struct AudioFrameIter<'a> {
    data: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> Iterator for AudioFrameIter<'a> {
    type Item = AudioFrame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + 12 > self.end {
            return None;
        }

        let timestamp_ms = u64::from_le_bytes(self.data[self.offset..self.offset + 8].try_into().ok()?);
        let data_len = u32::from_le_bytes(self.data[self.offset + 8..self.offset + 12].try_into().ok()?) as usize;

        let sample_start = self.offset + 12;
        let sample_end = sample_start + data_len;

        if sample_end > self.end {
            return None;
        }

        let samples = self.data[sample_start..sample_end].to_vec();
        self.offset = sample_end;

        Some(AudioFrame { timestamp_ms, samples })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_recording_path(name: &str) -> String {
        let dir = std::env::temp_dir().join("alldesk_test_recordings");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn test_recorder_write_and_read_roundtrip() {
        let path = temp_recording_path("roundtrip_v2.aldrec");

        let recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(4, 4)
            .with_fps(30);

        let mut recorder = recorder;
        let frame_data = vec![42u8; 64];
        recorder.write_frame(&frame_data, 0).unwrap();
        recorder.write_frame(&frame_data, 33).unwrap();
        recorder.write_frame(&frame_data, 66).unwrap();
        assert_eq!(recorder.frame_count(), 3);
        recorder.finish().unwrap();

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.width(), 4);
        assert_eq!(reader.height(), 4);
        assert_eq!(reader.fps(), 30);
        assert_eq!(reader.frame_count(), 3);

        let frames: Vec<_> = reader.frames().collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].0, 0);
        assert_eq!(frames[1].0, 33);
        assert_eq!(frames[2].0, 66);
        for (_, data) in &frames {
            assert_eq!(data.len(), 64);
            assert_eq!(data[0], 42);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_recorder_empty_recording() {
        let path = temp_recording_path("empty_v2.aldrec");

        let recorder = Recorder::new(&path).unwrap();
        assert_eq!(recorder.frame_count(), 0);
        recorder.finish().unwrap();

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.frame_count(), 0);
        let frames: Vec<_> = reader.frames().collect();
        assert!(frames.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_recorder_large_frame() {
        let path = temp_recording_path("large_frame_v2.aldrec");

        let mut recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(1920, 1080)
            .with_fps(60);

        let big_frame = vec![0xAB; 1920 * 1080 * 4];
        recorder.write_frame(&big_frame, 0).unwrap();
        recorder.finish().unwrap();

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.frame_count(), 1);
        let frames: Vec<_> = reader.frames().collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].1.len(), 1920 * 1080 * 4);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_reader_invalid_magic() {
        let path = temp_recording_path("bad_magic.aldrec");
        std::fs::write(&path, b"NOTREC\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00").unwrap();

        let result = RecordingReader::open(&path);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_reader_file_too_short() {
        let path = temp_recording_path("short.aldrec");
        std::fs::write(&path, b"ALDREC").unwrap();

        let result = RecordingReader::open(&path);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_reader_nonexistent_file() {
        let result = RecordingReader::open("/nonexistent/path.aldrec");
        assert!(result.is_err());
    }

    #[test]
    fn test_recorder_with_dimensions_and_fps() {
        let path = temp_recording_path("dims_v2.aldrec");

        let recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(640, 480)
            .with_fps(24);

        assert_eq!(recorder.width(), 640);
        assert_eq!(recorder.height(), 480);

        drop(recorder);

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.width(), 640);
        assert_eq!(reader.height(), 480);
        assert_eq!(reader.fps(), 24);

        let _ = std::fs::remove_file(&path);
    }

    // === Audio Tests ===

    #[test]
    fn test_audio_write_and_read_roundtrip() {
        let path = temp_recording_path("audio_roundtrip.aldrec");

        let recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(4, 4)
            .with_fps(30)
            .with_audio(48000);

        let mut recorder = recorder;
        assert_eq!(recorder.audio_sample_rate(), 48000);

        // Write video frame.
        let frame_data = vec![42u8; 64];
        recorder.write_frame(&frame_data, 0).unwrap();

        // Write audio frames.
        let audio_data = vec![0.5f32; 480].iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>();
        recorder.write_audio_frame(&audio_data, 0).unwrap();
        recorder.write_audio_frame(&audio_data, 10).unwrap();
        assert_eq!(recorder.audio_frame_count(), 2);

        recorder.finish().unwrap();

        // Read back.
        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.width(), 4);
        assert_eq!(reader.frame_count(), 1);
        assert_eq!(reader.audio_sample_rate(), 48000);
        assert_eq!(reader.audio_frame_count(), 2);

        // Check video frames.
        let video_frames: Vec<_> = reader.frames().collect();
        assert_eq!(video_frames.len(), 1);
        assert_eq!(video_frames[0].0, 0);

        // Check audio frames.
        let audio_frames: Vec<_> = reader.audio_frames().collect();
        assert_eq!(audio_frames.len(), 2);
        assert_eq!(audio_frames[0].timestamp_ms, 0);
        assert_eq!(audio_frames[1].timestamp_ms, 10);
        assert_eq!(audio_frames[0].samples.len(), 480 * 4); // 480 f32 samples

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_video_only_recording() {
        let path = temp_recording_path("video_only.aldrec");

        let recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(8, 8)
            .with_fps(30);

        let mut recorder = recorder;
        let frame_data = vec![0u8; 256]; // 8x8 BGRA
        recorder.write_frame(&frame_data, 0).unwrap();
        recorder.finish().unwrap();

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.audio_sample_rate(), 0);
        assert_eq!(reader.audio_frame_count(), 0);

        let audio_frames: Vec<_> = reader.audio_frames().collect();
        assert!(audio_frames.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_multiple_video_and_audio_frames() {
        let path = temp_recording_path("multi_av.aldrec");

        let recorder = Recorder::new(&path)
            .unwrap()
            .with_dimensions(2, 2)
            .with_fps(30)
            .with_audio(48000);

        let mut recorder = recorder;
        let frame = vec![0u8; 16]; // 2x2 BGRA
        let audio = vec![1.0f32; 48].iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>();

        // Write all video frames first, then audio frames.
        recorder.write_frame(&frame, 0).unwrap();
        recorder.write_frame(&frame, 33).unwrap();

        recorder.write_audio_frame(&audio, 0).unwrap();
        recorder.write_audio_frame(&audio, 10).unwrap();
        recorder.write_audio_frame(&audio, 20).unwrap();

        assert_eq!(recorder.frame_count(), 2);
        assert_eq!(recorder.audio_frame_count(), 3);
        recorder.finish().unwrap();

        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.frame_count(), 2);
        assert_eq!(reader.audio_frame_count(), 3);

        let video: Vec<_> = reader.frames().collect();
        assert_eq!(video.len(), 2);
        assert_eq!(video[0].0, 0);
        assert_eq!(video[1].0, 33);

        let audio: Vec<_> = reader.audio_frames().collect();
        assert_eq!(audio.len(), 3);
        assert_eq!(audio[0].timestamp_ms, 0);
        assert_eq!(audio[1].timestamp_ms, 10);
        assert_eq!(audio[2].timestamp_ms, 20);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_v1_backward_compatibility() {
        let path = temp_recording_path("v1_compat.aldrec");

        // Write a v1 file manually (old format, no audio).
        let mut header = [0u8; HEADER_V1_SIZE];
        header[0..6].copy_from_slice(MAGIC_V1);
        header[6..10].copy_from_slice(&320u32.to_le_bytes());
        header[10..14].copy_from_slice(&240u32.to_le_bytes());
        header[14..18].copy_from_slice(&30u32.to_le_bytes());
        header[18..22].copy_from_slice(&1u32.to_le_bytes()); // 1 frame

        let frame_data = vec![99u8; 320 * 240 * 4];
        let mut ts_bytes = 0u64.to_le_bytes();
        let len_bytes = (frame_data.len() as u32).to_le_bytes();

        let mut file_data = Vec::new();
        file_data.extend_from_slice(&header);
        file_data.extend_from_slice(&ts_bytes);
        file_data.extend_from_slice(&len_bytes);
        file_data.extend_from_slice(&frame_data);

        std::fs::write(&path, &file_data).unwrap();

        // Should read as v1 file with no audio.
        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.width(), 320);
        assert_eq!(reader.height(), 240);
        assert_eq!(reader.fps(), 30);
        assert_eq!(reader.frame_count(), 1);
        assert_eq!(reader.audio_sample_rate(), 0);

        let frames: Vec<_> = reader.frames().collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].1.len(), 320 * 240 * 4);

        let _ = std::fs::remove_file(&path);
    }
}
