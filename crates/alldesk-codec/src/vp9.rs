use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_long, c_ulong};
use std::ptr;
use std::slice;

use alldesk_capture::capture::{CapturedFrame, FrameData, PixelFormat};
use alldesk_core::{Error, Result};
use vpx_sys::*;

use super::decoder::{DecodedFrame, VideoDecoder};
use super::encoder::{Codec, EncodedPacket, VideoEncoder};

macro_rules! vpx_err {
    ($($arg:tt)*) => {
        Error::Codec(format!($($arg)*))
    };
}

macro_rules! vpx_call {
    ($x:expr) => {{
        let ret = unsafe { $x };
        if ret as i32 != 0 {
            return Err(vpx_err!("VPX call failed (code {})", ret as i32));
        }
        ret
    }};
}

macro_rules! vpx_ptr {
    ($x:expr) => {{
        let p = unsafe { $x };
        if p.is_null() {
            return Err(vpx_err!("VPX returned null pointer"));
        }
        p
    }};
}

// ---------- Color space conversion ----------

fn bgra_to_i420(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w / 2;
    let uv_height = h / 2;
    let uv_size = uv_stride * uv_height;

    let mut out = vec![0u8; y_size + 2 * uv_size];
    let (y_plane, rest) = out.split_at_mut(y_size);
    let (u_plane, v_plane) = rest.split_at_mut(uv_size);

    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * 4;
            let b = bgra[i] as i32;
            let g = bgra[i + 1] as i32;
            let r = bgra[i + 2] as i32;

            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[row * w + col] = y.clamp(0, 255) as u8;

            if row % 2 == 0 && col % 2 == 0 {
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                u_plane[(row / 2) * uv_stride + col / 2] = u.clamp(0, 255) as u8;
                v_plane[(row / 2) * uv_stride + col / 2] = v.clamp(0, 255) as u8;
            }
        }
    }

    out
}

fn i420_to_bgra(i420: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w / 2;
    let uv_size = uv_stride * (h / 2);

    let y_p = &i420[..y_size];
    let u_p = &i420[y_size..y_size + uv_size];
    let v_p = &i420[y_size + uv_size..];

    let mut out = vec![0u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let y = y_p[row * w + col] as i32;
            let u = u_p[(row / 2) * uv_stride + col / 2] as i32;
            let v = v_p[(row / 2) * uv_stride + col / 2] as i32;

            let c = y - 16;
            let d = u - 128;
            let e = v - 128;

            let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255);
            let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255);
            let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255);

            let i = (row * w + col) * 4;
            out[i] = b as u8;
            out[i + 1] = g as u8;
            out[i + 2] = r as u8;
            out[i + 3] = 255;
        }
    }

    out
}

// ---------- VP9 Encoder ----------

pub struct Vp9Encoder {
    ctx: vpx_codec_ctx_t,
    width: u32,
    height: u32,
    bitrate_kbps: u32,
    fps: u32,
    force_keyframe: bool,
}

// SAFETY: libvpx context is self-contained; all access is through &mut self.
unsafe impl Send for Vp9Encoder {}
unsafe impl Sync for Vp9Encoder {}

/// Realtime encoder config: every field set here must be set on *every*
/// reconfiguration too — `vpx_codec_enc_config_set` applies the whole struct,
/// so rebuilding from defaults without these would silently restore
/// `g_lag_in_frames` = 25 and reintroduce ~800 ms of latency.
fn realtime_config(
    width: u32,
    height: u32,
    bitrate_kbps: u32,
    fps: u32,
) -> Result<vpx_codec_enc_cfg_t> {
    let iface = vpx_ptr!(vpx_codec_vp9_cx());

    let mut cfg: vpx_codec_enc_cfg_t = unsafe { MaybeUninit::zeroed().assume_init() };
    vpx_call!(vpx_codec_enc_config_default(iface, &mut cfg, 0));

    cfg.g_w = width;
    cfg.g_h = height;
    cfg.g_timebase.num = 1;
    cfg.g_timebase.den = fps.max(1) as _;
    cfg.rc_target_bitrate = bitrate_kbps;
    cfg.g_threads = 4;
    cfg.g_error_resilient = VPX_ERROR_RESILIENT_DEFAULT as _;
    cfg.g_lag_in_frames = 0;
    Ok(cfg)
}

impl Vp9Encoder {
    pub fn new(width: u32, height: u32, bitrate_kbps: u32, fps: u32) -> Result<Self> {
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(vpx_err!("width/height must be divisible by 2"));
        }

        let iface = vpx_ptr!(vpx_codec_vp9_cx());

        let cfg = realtime_config(width, height, bitrate_kbps, fps)?;

        let mut ctx: vpx_codec_ctx_t = unsafe { MaybeUninit::zeroed().assume_init() };
        vpx_call!(vpx_codec_enc_init_ver(
            &mut ctx,
            iface,
            &cfg,
            0,
            VPX_ENCODER_ABI_VERSION as _,
        ));

        unsafe {
            vpx_codec_control_(
                &mut ctx,
                vp8e_enc_control_id::VP8E_SET_CPUUSED as _,
                6 as c_int,
            );
            vpx_codec_control_(
                &mut ctx,
                vp8e_enc_control_id::VP9E_SET_ROW_MT as _,
                1 as c_int,
            );
        }

        Ok(Self {
            ctx,
            width,
            height,
            bitrate_kbps,
            fps,
            force_keyframe: false,
        })
    }

    /// Live reconfiguration: apply a new bitrate and/or FPS without
    /// recreating the encoder context (rate-control state is preserved).
    /// Errors from libvpx are logged by the caller; on failure the encoder
    /// keeps running with its previous parameters.
    pub fn reconfigure(&mut self, bitrate_kbps: u32, fps: u32) {
        self.bitrate_kbps = bitrate_kbps;
        self.fps = fps;
        match realtime_config(self.width, self.height, bitrate_kbps, fps) {
            Ok(cfg) => unsafe {
                vpx_codec_enc_config_set(&mut self.ctx, &cfg);
            },
            Err(e) => tracing::warn!("VP9 reconfigure: {}", e),
        }
    }
}

impl VideoEncoder for Vp9Encoder {
    fn encode(&mut self, frame: &CapturedFrame) -> Result<Vec<EncodedPacket>> {
        let raw = match &frame.data {
            FrameData::Cpu(d) => d,
            FrameData::GpuTexture(_) => {
                return Err(vpx_err!(
                    "GPU texture not supported by VP9 software encoder"
                ));
            }
        };

        let i420 = match frame.format {
            PixelFormat::Bgra8888 => bgra_to_i420(raw, frame.width, frame.height),
            PixelFormat::Rgba8888 => {
                let mut bgra = raw.clone();
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                bgra_to_i420(&bgra, frame.width, frame.height)
            }
            PixelFormat::Nv12 => {
                return Err(vpx_err!("NV12 not yet supported"));
            }
        };

        let mut image: vpx_image_t = unsafe { MaybeUninit::zeroed().assume_init() };
        vpx_ptr!(vpx_img_wrap(
            &mut image,
            vpx_img_fmt::VPX_IMG_FMT_I420,
            self.width as _,
            self.height as _,
            1,
            i420.as_ptr() as _,
        ));

        let flags: c_long = if self.force_keyframe {
            self.force_keyframe = false;
            VPX_EFLAG_FORCE_KF as _
        } else {
            0
        };

        let pts = frame.timestamp.as_millis() as i64;

        let enc_ret = unsafe {
            vpx_codec_encode(
                &mut self.ctx,
                &image,
                pts,
                1,
                flags,
                VPX_DL_REALTIME as c_ulong,
            )
        };
        if enc_ret as u32 != 0 {
            return Err(vpx_err!("VP9 encode failed (code {})", enc_ret as u32));
        }

        let mut packets = Vec::new();
        let mut iter: vpx_codec_iter_t = ptr::null();

        loop {
            let pkt = unsafe { vpx_codec_get_cx_data(&mut self.ctx, &mut iter) };
            if pkt.is_null() {
                break;
            }
            let kind = unsafe { (*pkt).kind };
            if kind == vpx_codec_cx_pkt_kind::VPX_CODEC_CX_FRAME_PKT {
                unsafe {
                    let f = &(*pkt).data.frame;
                    let data = slice::from_raw_parts(f.buf as _, f.sz as usize);
                    let key = (f.flags & VPX_FRAME_IS_KEY) != 0;
                    packets.push(EncodedPacket {
                        data: data.to_vec(),
                        is_keyframe: key,
                        timestamp_ms: f.pts as u64,
                        codec: Codec::VP9,
                    });
                }
            }
        }

        Ok(packets)
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) {
        self.reconfigure(bitrate_kbps, self.fps);
    }

    fn request_key_frame(&mut self) {
        self.force_keyframe = true;
    }

    fn codec(&self) -> Codec {
        Codec::VP9
    }
}

impl Drop for Vp9Encoder {
    fn drop(&mut self) {
        unsafe {
            vpx_codec_destroy(&mut self.ctx);
        }
    }
}

// ---------- VP9 Decoder ----------

pub struct Vp9Decoder {
    ctx: vpx_codec_ctx_t,
    width: u32,
    height: u32,
}

unsafe impl Send for Vp9Decoder {}
unsafe impl Sync for Vp9Decoder {}

impl Vp9Decoder {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let iface = vpx_ptr!(vpx_codec_vp9_dx());

        let mut ctx: vpx_codec_ctx_t = unsafe { MaybeUninit::zeroed().assume_init() };
        vpx_call!(vpx_codec_dec_init_ver(
            &mut ctx,
            iface,
            ptr::null(),
            0,
            VPX_DECODER_ABI_VERSION as i32,
        ));

        Ok(Self { ctx, width, height })
    }
}

impl VideoDecoder for Vp9Decoder {
    fn decode(&mut self, packet: &EncodedPacket) -> Result<DecodedFrame> {
        vpx_call!(vpx_codec_decode(
            &mut self.ctx,
            packet.data.as_ptr(),
            packet.data.len() as _,
            ptr::null_mut(),
            0,
        ));

        let mut iter: vpx_codec_iter_t = ptr::null();
        unsafe {
            let img = vpx_codec_get_frame(&mut self.ctx, &mut iter);
            if img.is_null() {
                return Err(vpx_err!("no decoded frame"));
            }

            let img = &*img;
            let w = img.d_w as u32;
            let h = img.d_h as u32;

            let y_size = (w * h) as usize;
            let uv_size = ((w / 2) * (h / 2)) as usize;
            let mut i420 = vec![0u8; y_size + 2 * uv_size];

            for row in 0..h as usize {
                let src = img.planes[0].add(row * img.stride[0] as usize);
                let dst = row * w as usize;
                i420[dst..dst + w as usize].copy_from_slice(slice::from_raw_parts(src, w as usize));
            }
            for row in 0..(h / 2) as usize {
                let src = img.planes[1].add(row * img.stride[1] as usize);
                let dst = y_size + row * (w / 2) as usize;
                i420[dst..dst + (w / 2) as usize]
                    .copy_from_slice(slice::from_raw_parts(src, (w / 2) as usize));
            }
            for row in 0..(h / 2) as usize {
                let src = img.planes[2].add(row * img.stride[2] as usize);
                let dst = y_size + uv_size + row * (w / 2) as usize;
                i420[dst..dst + (w / 2) as usize]
                    .copy_from_slice(slice::from_raw_parts(src, (w / 2) as usize));
            }

            let bgra = i420_to_bgra(&i420, w, h);
            self.width = w;
            self.height = h;

            Ok(DecodedFrame {
                data: bgra,
                width: w,
                height: h,
                stride: w * 4,
            })
        }
    }
}

impl Drop for Vp9Decoder {
    fn drop(&mut self) {
        unsafe {
            vpx_codec_destroy(&mut self.ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_bgra_i420_roundtrip() {
        let w = 8u32;
        let h = 8u32;
        let bgra: Vec<u8> = (0..w * h * 4).map(|i| (i % 256) as u8).collect();
        let i420 = bgra_to_i420(&bgra, w, h);
        assert_eq!(i420.len(), (w * h * 3 / 2) as usize);

        let back = i420_to_bgra(&i420, w, h);
        assert_eq!(back.len(), (w * h * 4) as usize);
        for px in back.chunks_exact(4) {
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn test_vp9_encoder_new() {
        let enc = Vp9Encoder::new(320, 240, 500, 30);
        assert!(enc.is_ok(), "Vp9Encoder::new failed: {:?}", enc.err());
    }

    #[test]
    fn test_vp9_encode_decode() {
        let w = 320u32;
        let h = 240u32;
        let mut enc = Vp9Encoder::new(w, h, 500, 30).unwrap();
        let mut dec = Vp9Decoder::new(w, h).unwrap();

        let bgra: Vec<u8> = (0..w * h)
            .flat_map(|_| [100u8, 150u8, 200u8, 255u8])
            .collect();
        let frame = CapturedFrame {
            data: FrameData::Cpu(bgra),
            width: w,
            height: h,
            format: PixelFormat::Bgra8888,
            damage_regions: vec![],
            timestamp: Duration::from_millis(0),
            monitor_id: 0,
            cursor: None,
        };

        let packets = enc.encode(&frame).unwrap();
        assert!(!packets.is_empty(), "encoder should produce packets");
        assert!(packets[0].is_keyframe, "first packet should be keyframe");

        let decoded = dec.decode(&packets[0]).unwrap();
        assert_eq!(decoded.width, w);
        assert_eq!(decoded.height, h);
        assert_eq!(decoded.stride, w * 4);
    }

    #[test]
    fn test_vp9_set_bitrate() {
        let mut enc = Vp9Encoder::new(320, 240, 500, 30).unwrap();
        enc.set_bitrate(1000);
        assert_eq!(enc.bitrate_kbps, 1000);
    }

    #[test]
    fn test_vp9_request_keyframe() {
        let mut enc = Vp9Encoder::new(320, 240, 500, 30).unwrap();
        assert!(!enc.force_keyframe);
        enc.request_key_frame();
        assert!(enc.force_keyframe);
    }

    #[test]
    fn test_vp9_odd_dimensions_rejected() {
        assert!(Vp9Encoder::new(321, 240, 500, 30).is_err());
        assert!(Vp9Encoder::new(320, 241, 500, 30).is_err());
    }
}
