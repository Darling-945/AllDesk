//! Minimal WebM muxer for VP9 video streams.
//!
//! WebM is a subset of Matroska, which uses EBML (Extensible Binary Meta Language)
//! as its underlying format. This module implements just enough of the spec to
//! produce valid WebM files playable in VLC and Chrome.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use alldesk_core::Result;

// ---------------------------------------------------------------------------
// EBML / Matroska element IDs
// ---------------------------------------------------------------------------

/// EBML header root element.
const ID_EBML: u32 = 0x1A45DFA3;
const ID_EBML_VERSION: u32 = 0x4286;
const ID_EBML_READ_VERSION: u32 = 0x42F7;
const ID_EBML_MAX_ID_LENGTH: u32 = 0x42F2;
const ID_EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
const ID_DOC_TYPE: u32 = 0x4282;
const ID_DOC_TYPE_VERSION: u32 = 0x4287;
const ID_DOC_TYPE_READ_VERSION: u32 = 0x4285;

/// Segment (top-level container – size left as "unknown" 0x01FFFFFFFFFFFFFF).
const ID_SEGMENT: u32 = 0x18538067;

/// SegmentInfo child elements.
const ID_SEGMENT_INFO: u32 = 0x1549A966;
const ID_TIMECODE_SCALE: u32 = 0x2AD7B1;
const ID_MUXING_APP: u32 = 0x4D80;
const ID_WRITING_APP: u32 = 0x5741;

/// Tracks / TrackEntry elements.
const ID_TRACKS: u32 = 0x1654AE6B;
const ID_TRACK_ENTRY: u32 = 0xAE;
const ID_TRACK_NUMBER: u32 = 0xD7;
const ID_TRACK_UID: u32 = 0x73C5;
const ID_TRACK_TYPE: u32 = 0x83; // 1 = video
const ID_CODEC_ID: u32 = 0x86;
const ID_VIDEO: u32 = 0xE0;
const ID_PIXEL_WIDTH: u32 = 0xB0;
const ID_PIXEL_HEIGHT: u32 = 0xBA;
const ID_STEREO_MODE: u32 = 0x53B8;

/// Cluster elements.
const ID_CLUSTER: u32 = 0x1F43B675;
const ID_TIMECODE: u32 = 0xE7;
const ID_SIMPLE_BLOCK: u32 = 0xA3;

/// Unknown-size sentinel (8-byte VINT with all data bits set).
const UNKNOWN_SIZE: &[u8; 8] = &[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// Maximum number of milliseconds before starting a new cluster.
const CLUSTER_DURATION_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// EBML primitive helpers
// ---------------------------------------------------------------------------

/// Encode an unsigned integer as an EBML variable-length integer (VINT).
///
/// The VINT width is chosen automatically: 1-8 bytes depending on value.
/// Returns the encoded bytes.
fn encode_vint(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![0x80];
    }

    for width in 1..=8u8 {
        let data_bits = (width as usize) * 8 - width as usize;
        if value < (1u64 << data_bits) {
            let marker = 1u64 << data_bits;
            let encoded = marker | value;
            let nbytes = width as usize;
            let mut buf = Vec::with_capacity(nbytes);
            for i in 0..nbytes {
                buf.push((encoded >> ((nbytes - 1 - i) * 8)) as u8);
            }
            return buf;
        }
    }
    let encoded = 0x01_00000000_000000 | value;
    let nbytes = 8;
    let mut buf = Vec::with_capacity(nbytes);
    for i in 0..nbytes {
        buf.push((encoded >> ((nbytes - 1 - i) * 8)) as u8);
    }
    buf
}

/// Write an EBML element ID (always a VINT, but IDs have a fixed width
/// determined by the leading-one-bit position).
fn write_element_id(writer: &mut impl Write, id: u32) -> std::io::Result<()> {
    // Element IDs are stored as their raw big-endian representation.
    // Determine width from the MSB position.
    if id <= 0xFF {
        writer.write_all(&[id as u8])?;
    } else if id <= 0xFFFF {
        writer.write_all(&(id as u16).to_be_bytes())?;
    } else if id <= 0xFFFFFF {
        writer.write_all(&[(id >> 16) as u8, (id >> 8) as u8, id as u8])?;
    } else {
        writer.write_all(&id.to_be_bytes())?;
    }
    Ok(())
}

/// Write a complete EBML element: ID + VINT size + payload.
fn write_element(writer: &mut impl Write, id: u32, data: &[u8]) -> std::io::Result<()> {
    write_element_id(writer, id)?;
    let size = encode_vint(data.len() as u64);
    writer.write_all(&size)?;
    writer.write_all(data)?;
    Ok(())
}

/// Write an EBML unsigned-integer element (ID + size + big-endian value).
fn write_uint_element(writer: &mut impl Write, id: u32, value: u64) -> std::io::Result<()> {
    // Determine minimal byte width.
    let bytes = if value == 0 {
        vec![0u8]
    } else {
        let len = (64 - value.leading_zeros() + 7) / 8;
        (0..len).rev().map(|i| (value >> (i * 8)) as u8).collect::<Vec<_>>()
    };
    write_element(writer, id, &bytes)
}

/// Write an EBML string element.
fn write_string_element(writer: &mut impl Write, id: u32, s: &str) -> std::io::Result<()> {
    write_element(writer, id, s.as_bytes())
}

/// Write the EBML header that starts every WebM file.
fn write_ebml_header(writer: &mut impl Write) -> std::io::Result<()> {
    let mut payload = Vec::new();
    write_uint_element(&mut payload, ID_EBML_VERSION, 4)?;
    write_uint_element(&mut payload, ID_EBML_READ_VERSION, 4)?;
    write_uint_element(&mut payload, ID_EBML_MAX_ID_LENGTH, 4)?;
    write_uint_element(&mut payload, ID_EBML_MAX_SIZE_LENGTH, 8)?;
    write_string_element(&mut payload, ID_DOC_TYPE, "webm")?;
    write_uint_element(&mut payload, ID_DOC_TYPE_VERSION, 4)?;
    write_uint_element(&mut payload, ID_DOC_TYPE_READ_VERSION, 2)?;
    write_element(writer, ID_EBML, &payload)
}

/// Write the SegmentInfo element.
fn write_segment_info(writer: &mut impl Write) -> std::io::Result<()> {
    let mut payload = Vec::new();
    // TimecodeScale = 1 000 000 (nanoseconds per tick → 1 ms per tick).
    write_uint_element(&mut payload, ID_TIMECODE_SCALE, 1_000_000)?;
    write_string_element(&mut payload, ID_MUXING_APP, "alldesk")?;
    write_string_element(&mut payload, ID_WRITING_APP, "alldesk")?;
    write_element(writer, ID_SEGMENT_INFO, &payload)
}

/// Write the Tracks element with a single VP9 video track.
fn write_tracks(writer: &mut impl Write, width: u32, height: u32) -> std::io::Result<()> {
    let mut track = Vec::new();
    write_uint_element(&mut track, ID_TRACK_NUMBER, 1)?;
    write_uint_element(&mut track, ID_TRACK_UID, 1)?;
    write_uint_element(&mut track, ID_TRACK_TYPE, 1)?; // video

    let codec_id = b"V_VP9";
    write_element(&mut track, ID_CODEC_ID, codec_id)?;

    // Video settings.
    let mut video = Vec::new();
    write_uint_element(&mut video, ID_PIXEL_WIDTH, width as u64)?;
    write_uint_element(&mut video, ID_PIXEL_HEIGHT, height as u64)?;
    write_uint_element(&mut video, ID_STEREO_MODE, 0)?; // mono
    write_element(&mut track, ID_VIDEO, &video)?;

    let mut tracks = Vec::new();
    write_element(&mut tracks, ID_TRACK_ENTRY, &track)?;
    write_element(writer, ID_TRACKS, &tracks)
}

// ---------------------------------------------------------------------------
// WebmMuxer
// ---------------------------------------------------------------------------

/// Minimal WebM muxer that writes a VP9 video stream into a valid WebM container.
///
/// Usage:
/// ```ignore
/// let mut muxer = WebmMuxer::new("recording.webm", 1920, 1080, 30)?;
/// for frame in vp9_frames {
///     muxer.write_frame(&frame.data, frame.timestamp_ms, frame.is_keyframe)?;
/// }
/// muxer.finish()?;
/// ```
pub struct WebmMuxer {
    file: std::fs::File,
    width: u32,
    height: u32,
    fps: u32,
    frame_count: u32,
    /// Byte offset where the current Cluster's size field lives (so we can patch it on close).
    cluster_start: u64,
    /// The absolute timecode (ms) of the current cluster's first frame.
    cluster_timecode: u64,
    /// Number of frames written into the current cluster.
    frames_in_cluster: u32,
}

impl WebmMuxer {
    /// Create a new WebM file and write the header, SegmentInfo, and Tracks.
    ///
    /// After construction the file is positioned inside the Segment, ready for
    /// Cluster elements.
    pub fn new(path: &str, width: u32, height: u32, fps: u32) -> Result<Self> {
        let p = Path::new(path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut file = std::fs::File::create(p)?;

        // 1. EBML header.
        write_ebml_header(&mut file)?;

        // 2. Begin Segment with unknown size.
        write_element_id(&mut file, ID_SEGMENT)?;
        file.write_all(UNKNOWN_SIZE)?;

        // 3. SegmentInfo.
        write_segment_info(&mut file)?;

        // 4. Tracks.
        write_tracks(&mut file, width, height)?;

        file.flush()?;

        Ok(Self {
            file,
            width,
            height,
            fps,
            frame_count: 0,
            cluster_start: 0,
            cluster_timecode: 0,
            frames_in_cluster: 0,
        })
    }

    /// Write a single VP9 frame as a SimpleBlock.
    ///
    /// `timestamp_ms` is the absolute presentation time in milliseconds.
    /// `keyframe` should be `true` for VP9 keyframes.
    ///
    /// A new Cluster is started automatically every 5 seconds or when a
    /// keyframe arrives after the cluster threshold has been exceeded.
    pub fn write_frame(&mut self, data: &[u8], timestamp_ms: u64, keyframe: bool) -> Result<()> {
        // Start a new cluster if this is the very first frame, or we have exceeded
        // the cluster duration threshold.
        if self.frame_count == 0
            || self.frames_in_cluster == 0
            || (timestamp_ms - self.cluster_timecode) >= CLUSTER_DURATION_MS
        {
            self.start_cluster(timestamp_ms)?;
        }

        // Build SimpleBlock.
        // Format: TrackNumber (VINT) | Timecode (i16 BE) | Flags (u8) | Frame data
        let track_num = encode_vint(1); // track 1
        let relative_tc = (timestamp_ms - self.cluster_timecode) as i16;
        let flags: u8 = if keyframe { 0x80 } else { 0x00 };

        let mut block = Vec::with_capacity(track_num.len() + 2 + 1 + data.len());
        block.extend_from_slice(&track_num);
        block.extend_from_slice(&relative_tc.to_be_bytes());
        block.push(flags);
        block.extend_from_slice(data);

        write_element(&mut self.file, ID_SIMPLE_BLOCK, &block)?;

        self.frame_count += 1;
        self.frames_in_cluster += 1;

        // Periodic flush.
        if self.frame_count % 30 == 0 {
            self.file.flush()?;
        }

        Ok(())
    }

    /// Start a new Cluster. If a previous cluster was open its size is left
    /// as-is (we rely on the unknown-size Segment so individual Cluster sizes
    /// just need to be correct – we write them using a generous fixed-size VINT
    /// placeholder and patch on [`finish`](Self::finish)).
    fn start_cluster(&mut self, timecode_ms: u64) -> std::io::Result<()> {
        // Record position of this cluster so we can patch its size later.
        let cluster_id_pos = self.file.stream_position()?;

        // Write Cluster element header: ID + size placeholder.
        write_element_id(&mut self.file, ID_CLUSTER)?;
        // Use a 5-byte VINT for the size placeholder: marker 0x08 + 4 data bytes.
        // This gives us up to ~4 GB per cluster which is more than enough.
        self.file.write_all(&[0x08, 0x00, 0x00, 0x00, 0x00])?;

        // Write the cluster-level Timecode (absolute ms for this cluster).
        write_uint_element(&mut self.file, ID_TIMECODE, timecode_ms)?;

        self.cluster_timecode = timecode_ms;
        self.frames_in_cluster = 0;
        self.cluster_start = cluster_id_pos;
        Ok(())
    }

    /// Finalize the WebM file: patch the last Cluster size and flush.
    pub fn finish(mut self) -> Result<()> {
        if self.frame_count > 0 && self.cluster_start > 0 {
            self.patch_last_cluster_size()?;
        }
        self.file.flush()?;
        Ok(())
    }

    /// Patch the size of the last Cluster element so decoders can determine
    /// its boundary even though the Segment uses unknown size.
    fn patch_last_cluster_size(&mut self) -> std::io::Result<()> {
        let current_pos = self.file.stream_position()?;
        let cluster_data_start = self.cluster_start
            // Skip past the Cluster element ID (variable width, at most 4 bytes).
            + element_id_size(ID_CLUSTER) as u64
            // Skip past the 5-byte size placeholder we wrote.
            + 5;
        let cluster_data_len = current_pos - cluster_data_start;

        // Seek back to the size field (just after the element ID).
        let size_field_pos = self.cluster_start + element_id_size(ID_CLUSTER) as u64;
        self.file.seek(SeekFrom::Start(size_field_pos))?;

        // Encode the actual size as a 5-byte VINT.
        let size_vint = encode_vint_fixed5(cluster_data_len);
        self.file.write_all(&size_vint)?;

        // Seek back to end.
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    /// Returns the total number of frames written so far.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Returns the configured video width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the configured video height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the configured FPS.
    pub fn fps(&self) -> u32 {
        self.fps
    }
}

/// Return the byte length of an element ID when written in its minimal form.
fn element_id_size(id: u32) -> usize {
    if id <= 0xFF {
        1
    } else if id <= 0xFFFF {
        2
    } else if id <= 0xFFFFFF {
        3
    } else {
        4
    }
}

/// Encode `value` as a fixed-width 5-byte VINT (marker byte 0x08).
///
/// 5-byte VINT layout: bit pattern `10000 dddd dddddddd dddddddd dddddddd`
/// Total data bits = 32. Maximum value = 2^32 - 2 = 4294967294.
fn encode_vint_fixed5(value: u64) -> [u8; 5] {
    assert!(value < (1u64 << 32));
    // Marker byte: 0x08 (binary 00001000) with top bit implicitly set to 1 for VINT width=5.
    // Actually EBML VINT: width-5 marker = 0001 0000 = 0x10? No.
    // Let me be precise: for width=5, the marker pattern has the leading 1 at bit position
    // (8*5 - 5) = 35 counting from bit 0 of the 5-byte sequence.
    // But we encode byte-by-byte: the first byte has the marker.
    // Width-1: 1xxxxxxx (0x80)
    // Width-2: 01xxxxxx (0x40..)
    // Width-3: 001xxxxx (0x20..)
    // Width-4: 0001xxxx (0x10..)
    // Width-5: 00001xxx (0x08..)
    // So the marker byte for width 5 is 0x08 with the lower 3 bits as data.
    // Since value < 2^32, the top 3 bits of value (bits 32-34) are always 0.
    let b0 = 0x08; // top 3 data bits are always 0 for values < 2^32
    let b1 = ((value >> 24) & 0xFF) as u8;
    let b2 = ((value >> 16) & 0xFF) as u8;
    let b3 = ((value >> 8) & 0xFF) as u8;
    let b4 = (value & 0xFF) as u8;
    [b0, b1, b2, b3, b4]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_webm_path(name: &str) -> String {
        let dir = std::env::temp_dir().join("alldesk_test_webm");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn test_webm_muxer_creates_file() {
        let path = temp_webm_path("creates_file.webm");
        let muxer = WebmMuxer::new(&path, 640, 480, 30).unwrap();
        assert_eq!(muxer.width(), 640);
        assert_eq!(muxer.height(), 480);
        assert_eq!(muxer.fps(), 30);
        assert_eq!(muxer.frame_count(), 0);
        muxer.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        // File should start with the EBML header element ID.
        assert!(data.len() > 4);
        assert_eq!(&data[0..4], &0x1A45DFA3u32.to_be_bytes());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webm_ebml_header() {
        let path = temp_webm_path("ebml_header.webm");
        let muxer = WebmMuxer::new(&path, 320, 240, 30).unwrap();
        muxer.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        // Verify EBML header element ID at offset 0.
        assert_eq!(&data[0..4], &0x1A45DFA3u32.to_be_bytes());

        // Verify that "webm" DocType string appears in the file.
        let doc_type_str = b"webm";
        assert!(
            data.windows(doc_type_str.len()).any(|w| w == doc_type_str),
            "DocType 'webm' not found in file"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webm_vint_encoding() {
        // EBML VINT encoding:
        //   Width 1: 1xxxxxxx (7 data bits, values 0-127)
        //   Width 2: 01xxxxxx xxxxxxxx (14 data bits, values 0-16383)
        //   Width 3: 001xxxxx xxxxxxxx xxxxxxxx (21 data bits)

        // Value 0: single byte 0x80 (marker bit + 0 data bits).
        assert_eq!(encode_vint(0), vec![0x80]);

        // Value 1: single byte 0x81.
        assert_eq!(encode_vint(1), vec![0x81]);

        // Value 126: 0xFE (0x80 | 126 = 0x80 | 0x7E).
        assert_eq!(encode_vint(126), vec![0xFE]);

        // Value 127: still fits in width 1 (max 7 data bits = 127).
        assert_eq!(encode_vint(127), vec![0xFF]);

        // Value 128: needs width 2. 0x40 | (128 >> 8) = 0x40, byte1 = 0x80.
        assert_eq!(encode_vint(128), vec![0x40, 0x80]);

        // Max width-2: 16383 = 0x3FFF → 0x7F 0xFF.
        assert_eq!(encode_vint(16383), vec![0x7F, 0xFF]);

        // 16384 needs width 3: marker 0x20, data = 0x4000.
        let v = encode_vint(16384);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], 0x20);
        assert_eq!(v[1], 0x40);
        assert_eq!(v[2], 0x00);

        // A large value (e.g. 1_000_000) to verify multi-byte works.
        let v = encode_vint(1_000_000);
        assert!(v.len() >= 3);
        // Verify round-trip by decoding.
        let decoded = decode_vint_for_test(&v);
        assert_eq!(decoded, 1_000_000);
    }

    /// Helper to decode a VINT for test verification.
    fn decode_vint_for_test(bytes: &[u8]) -> u64 {
        let first = bytes[0];
        let w = match first {
            f if f & 0x80 != 0 => 1,
            f if f & 0x40 != 0 => 2,
            f if f & 0x20 != 0 => 3,
            f if f & 0x10 != 0 => 4,
            f if f & 0x08 != 0 => 5,
            f if f & 0x04 != 0 => 6,
            f if f & 0x02 != 0 => 7,
            f if f & 0x01 != 0 => 8,
            _ => unreachable!(),
        };
        // Clear the marker bit.
        let marker_bit = 1u8 << (8 - w);
        let mut value = ((first & !marker_bit) as u64) << ((w - 1) * 8);
        for i in 1..w {
            value |= (bytes[i] as u64) << ((w - 1 - i) * 8);
        }
        value
    }

    #[test]
    fn test_webm_write_multiple_frames() {
        let path = temp_webm_path("multiple_frames.webm");

        let mut muxer = WebmMuxer::new(&path, 1920, 1080, 30).unwrap();

        // Write a sequence of frames simulating a VP9 stream.
        // Use dummy data; in real usage this would be VP9-encoded frames.
        let fake_keyframe = vec![0u8; 100];
        let fake_delta = vec![1u8; 50];

        muxer.write_frame(&fake_keyframe, 0, true).unwrap();
        muxer.write_frame(&fake_delta, 33, false).unwrap();
        muxer.write_frame(&fake_delta, 66, false).unwrap();
        muxer.write_frame(&fake_delta, 100, false).unwrap();

        assert_eq!(muxer.frame_count(), 4);
        muxer.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 100);

        // Verify SimpleBlock element IDs appear in the file.
        // SimpleBlock ID = 0xA3.
        let simple_block_id = &[0xA3];
        let count = data.windows(1).filter(|w| w == simple_block_id).count();
        // We wrote 4 frames, should find at least 4 SimpleBlock IDs.
        assert!(count >= 4, "Expected at least 4 SimpleBlock elements, found {}", count);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webm_segment_info() {
        let path = temp_webm_path("segment_info.webm");
        let muxer = WebmMuxer::new(&path, 800, 600, 24).unwrap();
        muxer.finish().unwrap();

        let data = std::fs::read(&path).unwrap();

        // Verify SegmentInfo element ID appears.
        let seg_info_id = &0x1549A966u32.to_be_bytes();
        assert!(
            data.windows(4).any(|w| w == seg_info_id),
            "SegmentInfo element not found"
        );

        // Verify the TimecodeScale value (1_000_000 = 0x0F4240).
        // It's encoded as 3 bytes big-endian: [0x0F, 0x42, 0x40].
        let tcs_bytes = &[0x0F, 0x42, 0x40];
        assert!(
            data.windows(3).any(|w| w == tcs_bytes),
            "TimecodeScale value not found"
        );

        // Verify MuxingApp string.
        let app_str = b"alldesk";
        assert!(
            data.windows(app_str.len()).any(|w| w == app_str),
            "MuxingApp/WritingApp string not found"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_webm_finish() {
        let path = temp_webm_path("finish.webm");

        let mut muxer = WebmMuxer::new(&path, 1280, 720, 60).unwrap();

        // Write frames spanning more than 5 seconds to force multiple clusters.
        let keyframe = vec![0xAB; 200];
        let delta = vec![0xCD; 80];

        // Frame at 0ms – new cluster.
        muxer.write_frame(&keyframe, 0, true).unwrap();
        // Frames within the same 5-second window.
        for ts in (33..5000).step_by(33) {
            muxer.write_frame(&delta, ts, false).unwrap();
        }
        // Frame at 5000ms – should trigger a new cluster.
        muxer.write_frame(&keyframe, 5000, true).unwrap();
        // A few more frames in the second cluster.
        muxer.write_frame(&delta, 5033, false).unwrap();
        muxer.write_frame(&delta, 5066, false).unwrap();

        let frame_count = muxer.frame_count();
        assert!(frame_count > 150, "Expected >150 frames, got {}", frame_count);

        muxer.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 1000);

        // Verify that at least two Cluster element IDs appear (we wrote >5s of data).
        let cluster_id = &0x1F43B675u32.to_be_bytes();
        let cluster_count = data.windows(4).filter(|w| w == cluster_id).count();
        assert!(
            cluster_count >= 2,
            "Expected at least 2 Cluster elements for >5s recording, found {}",
            cluster_count
        );

        let _ = std::fs::remove_file(&path);
    }
}
