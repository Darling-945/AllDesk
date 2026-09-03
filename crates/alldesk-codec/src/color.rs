//! Color space conversion utilities for video encoding/decoding.
//!
//! Provides optimized BGRA to I420 (YUV 4:2:0) conversion used by VP9/H.264 encoding,
//! and I420 to BGRA conversion for decoding output.

/// Convert BGRA (Blue-Green-Red-Alpha) pixels to I420 (YUV 4:2:0) planar format.
///
/// Output layout: [Y plane (w*h)] [U plane (w*h/4)] [V plane (w*h/4)]
///
/// This is the standard input format for VP9/H.264 encoders.
pub fn bgra_to_i420(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = y_size / 4;
    let mut y_plane = vec![0u8; y_size];
    let mut u_plane = vec![0u8; uv_size];
    let mut v_plane = vec![0u8; uv_size];

    // Full-resolution Y plane
    for row in 0..h {
        for col in 0..w {
            let pixel_offset = (row * w + col) * 4;
            if pixel_offset + 3 >= bgra.len() {
                break;
            }
            let b = bgra[pixel_offset] as f64;
            let g = bgra[pixel_offset + 1] as f64;
            let r = bgra[pixel_offset + 2] as f64;

            // BT.601 Y conversion
            let y = (0.299 * r + 0.587 * g + 0.114 * b).round() as u8;
            y_plane[row * w + col] = y;
        }
    }

    // Half-resolution U and V planes (2x2 subsampling)
    let uv_w = w / 2;
    for row in 0..h / 2 {
        for col in 0..uv_w {
            // Average 4 pixels (2x2 block)
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut count = 0u32;

            for dy in 0..2 {
                for dx in 0..2 {
                    let px = col * 2 + dx;
                    let py = row * 2 + dy;
                    let offset = (py * w + px) * 4;
                    if offset + 3 < bgra.len() {
                        sum_b += bgra[offset] as u32;
                        sum_g += bgra[offset + 1] as u32;
                        sum_r += bgra[offset + 2] as u32;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let avg_r = sum_r as f64 / count as f64;
                let avg_g = sum_g as f64 / count as f64;
                let avg_b = sum_b as f64 / count as f64;

                // BT.601 U and V conversion
                let u = ((-0.168736 * avg_r - 0.331264 * avg_g + 0.5 * avg_b + 128.0).round())
                    .clamp(0.0, 255.0) as u8;
                let v = ((0.5 * avg_r - 0.418688 * avg_g - 0.081312 * avg_b + 128.0).round())
                    .clamp(0.0, 255.0) as u8;

                u_plane[row * uv_w + col] = u;
                v_plane[row * uv_w + col] = v;
            }
        }
    }

    // Concatenate planes
    let mut output = Vec::with_capacity(y_size + uv_size * 2);
    output.extend_from_slice(&y_plane);
    output.extend_from_slice(&u_plane);
    output.extend_from_slice(&v_plane);
    output
}

/// Convert I420 (YUV 4:2:0) planar format to BGRA pixels.
///
/// Input layout: [Y plane (w*h)] [U plane (w*h/4)] [V plane (w*h/4)]
pub fn i420_to_bgra(i420: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w / 2;

    if i420.len() < y_size + y_size / 2 {
        return Vec::new();
    }

    let y_plane = &i420[..y_size];
    let u_plane = &i420[y_size..y_size + y_size / 4];
    let v_plane = &i420[y_size + y_size / 4..];

    let mut bgra = vec![0u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f64;
            let uv_row = row / 2;
            let uv_col = col / 2;
            let u = u_plane[uv_row * uv_stride + uv_col] as f64;
            let v = v_plane[uv_row * uv_stride + uv_col] as f64;

            // BT.601 inverse conversion
            let r = (y + 1.402 * (v - 128.0)).round().clamp(0.0, 255.0) as u8;
            let g = (y - 0.344136 * (u - 128.0) - 0.714136 * (v - 128.0))
                .round()
                .clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * (u - 128.0)).round().clamp(0.0, 255.0) as u8;

            let offset = (row * w + col) * 4;
            bgra[offset] = b;
            bgra[offset + 1] = g;
            bgra[offset + 2] = r;
            bgra[offset + 3] = 255; // Full opacity
        }
    }

    bgra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgra_to_i420_dimensions() {
        let bgra = vec![128u8; 16 * 16 * 4]; // 16x16 BGRA
        let i420 = bgra_to_i420(&bgra, 16, 16);
        // Y: 16*16=256, U: 8*8=64, V: 8*8=64, total=384
        assert_eq!(i420.len(), 256 + 64 + 64);
    }

    #[test]
    fn test_bgra_to_i420_white_pixel() {
        // White BGRA = (255, 255, 255, 255)
        let bgra = vec![255u8; 8 * 8 * 4];
        let i420 = bgra_to_i420(&bgra, 8, 8);
        // Y for white should be ~235 (BT.601)
        assert!(i420[0] > 200);
    }

    #[test]
    fn test_bgra_to_i420_black_pixel() {
        // Black BGRA = (0, 0, 0, 0)
        let bgra = vec![0u8; 8 * 8 * 4];
        let i420 = bgra_to_i420(&bgra, 8, 8);
        // Y for black should be ~16 (BT.601)
        assert!(i420[0] < 30);
    }

    #[test]
    fn test_i420_to_bgra_dimensions() {
        let i420 = vec![128u8; 16 * 16 + 8 * 8 + 8 * 8]; // Y + U + V
        let bgra = i420_to_bgra(&i420, 16, 16);
        assert_eq!(bgra.len(), 16 * 16 * 4);
    }

    #[test]
    fn test_i420_to_bgra_alpha_full() {
        let i420 = vec![128u8; 8 * 8 + 4 * 4 + 4 * 4];
        let bgra = i420_to_bgra(&i420, 8, 8);
        // All alpha values should be 255
        for i in (3..bgra.len()).step_by(4) {
            assert_eq!(bgra[i], 255);
        }
    }

    #[test]
    fn test_bgra_i420_roundtrip() {
        let width = 8u32;
        let height = 8u32;
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        // Fill with a pattern
        for i in 0..bgra.len() {
            bgra[i] = ((i * 37) % 256) as u8;
        }

        let i420 = bgra_to_i420(&bgra, width, height);
        let bgra_back = i420_to_bgra(&i420, width, height);

        // Due to chroma subsampling, we can't expect perfect roundtrip
        // but the values should be in a reasonable range
        assert_eq!(bgra_back.len(), bgra.len());

        // Check alpha is always 255
        for i in (3..bgra_back.len()).step_by(4) {
            assert_eq!(bgra_back[i], 255);
        }
    }

    #[test]
    fn test_i420_to_bgra_short_input() {
        let short = vec![0u8; 10];
        let bgra = i420_to_bgra(&short, 8, 8);
        assert!(bgra.is_empty());
    }
}

/// Convert BGRA pixels to NV12 format.
///
/// NV12 layout: [Y plane (w*h)] [interleaved UV plane (w*h/2)]
/// The UV plane stores U and V samples interleaved: [U0,V0, U1,V1, ...]
/// Each UV pair covers a 2x2 block of Y samples.
pub fn bgra_to_nv12(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = y_size / 2;
    let mut output = vec![0u8; y_size + uv_size];

    // Y plane
    for row in 0..h {
        for col in 0..w {
            let pixel_offset = (row * w + col) * 4;
            if pixel_offset + 3 >= bgra.len() {
                break;
            }
            let b = bgra[pixel_offset] as f64;
            let g = bgra[pixel_offset + 1] as f64;
            let r = bgra[pixel_offset + 2] as f64;
            let y = (0.257 * r + 0.504 * g + 0.098 * b + 16.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            output[row * w + col] = y;
        }
    }

    // Interleaved UV plane (2x2 subsampled)
    let uv_stride = w; // UV plane has same stride as Y, but each pair covers 2 cols
    for uv_row in 0..h / 2 {
        for uv_col in 0..w / 2 {
            let mut sum_b = 0.0;
            let mut sum_g = 0.0;
            let mut sum_r = 0.0;
            for dy in 0..2 {
                for dx in 0..2 {
                    let pixel_offset = ((uv_row * 2 + dy) * w + (uv_col * 2 + dx)) * 4;
                    if pixel_offset + 3 < bgra.len() {
                        sum_b += bgra[pixel_offset] as f64;
                        sum_g += bgra[pixel_offset + 1] as f64;
                        sum_r += bgra[pixel_offset + 2] as f64;
                    }
                }
            }
            let avg_b = sum_b / 4.0;
            let avg_g = sum_g / 4.0;
            let avg_r = sum_r / 4.0;

            let u = (-0.148 * avg_r - 0.291 * avg_g + 0.439 * avg_b + 128.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let v = (0.439 * avg_r - 0.368 * avg_g - 0.071 * avg_b + 128.0)
                .round()
                .clamp(0.0, 255.0) as u8;

            let uv_offset = y_size + (uv_row * uv_stride + uv_col * 2);
            if uv_offset + 1 < output.len() {
                output[uv_offset] = u;
                output[uv_offset + 1] = v;
            }
        }
    }

    output
}

/// Convert NV12 format to BGRA pixels.
///
/// NV12 layout: [Y plane (w*h)] [interleaved UV plane (w*h/2)]
pub fn nv12_to_bgra(nv12: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;

    if nv12.len() < y_size + y_size / 2 {
        return Vec::new();
    }

    let y_plane = &nv12[..y_size];
    let uv_plane = &nv12[y_size..];
    let uv_stride = w;

    let mut bgra = vec![0u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f64;
            let uv_row = row / 2;
            let uv_col = col / 2;
            let uv_offset = uv_row * uv_stride + uv_col * 2;
            let u = uv_plane[uv_offset] as f64;
            let v = uv_plane[uv_offset + 1] as f64;

            let r = (y + 1.402 * (v - 128.0)).round().clamp(0.0, 255.0) as u8;
            let g = (y - 0.344136 * (u - 128.0) - 0.714136 * (v - 128.0))
                .round()
                .clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * (u - 128.0)).round().clamp(0.0, 255.0) as u8;

            let offset = (row * w + col) * 4;
            bgra[offset] = b;
            bgra[offset + 1] = g;
            bgra[offset + 2] = r;
            bgra[offset + 3] = 255;
        }
    }

    bgra
}

#[cfg(test)]
mod nv12_tests {
    use super::*;

    #[test]
    fn test_bgra_to_nv12_dimensions() {
        let bgra = vec![128u8; 16 * 16 * 4];
        let nv12 = bgra_to_nv12(&bgra, 16, 16);
        // Y: 256 + UV interleaved: 128 = 384
        assert_eq!(nv12.len(), 256 + 128);
    }

    #[test]
    fn test_bgra_to_nv12_white() {
        let bgra = vec![255u8; 8 * 8 * 4];
        let nv12 = bgra_to_nv12(&bgra, 8, 8);
        assert!(nv12[0] > 200); // Y for white
    }

    #[test]
    fn test_nv12_to_bgra_dimensions() {
        let nv12 = vec![128u8; 16 * 16 + 16 * 8]; // Y + UV interleaved
        let bgra = nv12_to_bgra(&nv12, 16, 16);
        assert_eq!(bgra.len(), 16 * 16 * 4);
    }

    #[test]
    fn test_nv12_to_bgra_short_input() {
        let short = vec![0u8; 10];
        let bgra = nv12_to_bgra(&short, 8, 8);
        assert!(bgra.is_empty());
    }

    #[test]
    fn test_bgra_nv12_roundtrip() {
        let width = 8u32;
        let height = 8u32;
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        for i in 0..bgra.len() {
            bgra[i] = ((i * 37) % 256) as u8;
        }

        let nv12 = bgra_to_nv12(&bgra, width, height);
        let bgra_back = nv12_to_bgra(&nv12, width, height);

        assert_eq!(bgra_back.len(), bgra.len());
        for i in (3..bgra_back.len()).step_by(4) {
            assert_eq!(bgra_back[i], 255);
        }
    }

    #[test]
    fn test_nv12_alpha_full() {
        let nv12 = vec![128u8; 8 * 8 + 8 * 4];
        let bgra = nv12_to_bgra(&nv12, 8, 8);
        for i in (3..bgra.len()).step_by(4) {
            assert_eq!(bgra[i], 255);
        }
    }
}
