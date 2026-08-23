// SPDX-License-Identifier: AGPL-3.0-only

//! Fast pixel format conversion and downsampling for real-time video preview.

#[inline]
fn clamp_u8(val: i32) -> u8 {
    val.clamp(0, 255) as u8
}

#[inline]
fn yuv_to_rgb(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let c = y - 16;
    let d = u - 128;
    let e = v - 128;

    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;

    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Convert YUY2 (Y0 U0 Y1 V0 packed 4:2:2) to RGB24 with downsampling.
#[must_use]
pub fn convert_yuy2_to_rgb24(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Vec::new();
    }
    let mut dst = vec![0_u8; dst_width * dst_height * 3];
    let src_stride = src_width * 2;

    for dy in 0..dst_height {
        let sy = (dy * src_height) / dst_height;
        let row_start = sy * src_stride;
        let dst_row_start = dy * dst_width * 3;

        for dx in 0..dst_width {
            let sx = (dx * src_width) / dst_width;
            let pair_index = (sx / 2) * 4;
            let src_idx = row_start + pair_index;

            if src_idx + 3 < src.len() {
                let y = if sx % 2 == 0 {
                    i32::from(src[src_idx])
                } else {
                    i32::from(src[src_idx + 2])
                };
                let u = i32::from(src[src_idx + 1]);
                let v = i32::from(src[src_idx + 3]);

                let (r, g, b) = yuv_to_rgb(y, u, v);
                let out_idx = dst_row_start + dx * 3;
                dst[out_idx] = r;
                dst[out_idx + 1] = g;
                dst[out_idx + 2] = b;
            }
        }
    }
    dst
}

/// Convert NV12 (Y plane followed by interleaved UV plane) to RGB24 with downsampling.
#[must_use]
pub fn convert_nv12_to_rgb24(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Vec::new();
    }
    let y_plane_size = src_width * src_height;
    if src.len() < y_plane_size + (y_plane_size / 2) {
        return Vec::new();
    }

    let mut dst = vec![0_u8; dst_width * dst_height * 3];
    let uv_plane_offset = y_plane_size;

    for dy in 0..dst_height {
        let sy = (dy * src_height) / dst_height;
        let y_row = sy * src_width;
        let uv_row = (sy / 2) * src_width;
        let dst_row_start = dy * dst_width * 3;

        for dx in 0..dst_width {
            let sx = (dx * src_width) / dst_width;
            let y_idx = y_row + sx;
            let uv_idx = uv_plane_offset + uv_row + (sx / 2) * 2;

            if y_idx < y_plane_size && uv_idx + 1 < src.len() {
                let y = i32::from(src[y_idx]);
                let u = i32::from(src[uv_idx]);
                let v = i32::from(src[uv_idx + 1]);

                let (r, g, b) = yuv_to_rgb(y, u, v);
                let out_idx = dst_row_start + dx * 3;
                dst[out_idx] = r;
                dst[out_idx + 1] = g;
                dst[out_idx + 2] = b;
            }
        }
    }
    dst
}

/// Downsample RGB24 to target resolution RGB24.
#[must_use]
pub fn downsample_rgb24(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Vec::new();
    }
    let mut dst = vec![0_u8; dst_width * dst_height * 3];
    let src_stride = src_width * 3;

    for dy in 0..dst_height {
        let sy = (dy * src_height) / dst_height;
        let row_start = sy * src_stride;
        let dst_row_start = dy * dst_width * 3;

        for dx in 0..dst_width {
            let sx = (dx * src_width) / dst_width;
            let src_idx = row_start + sx * 3;

            if src_idx + 2 < src.len() {
                let out_idx = dst_row_start + dx * 3;
                dst[out_idx] = src[src_idx];
                dst[out_idx + 1] = src[src_idx + 1];
                dst[out_idx + 2] = src[src_idx + 2];
            }
        }
    }
    dst
}

/// Convert BGRA/RGBA (32-bit) to RGB24 with downsampling.
#[must_use]
pub fn convert_rgb32_to_rgb24(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
    is_bgra: bool,
) -> Vec<u8> {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Vec::new();
    }
    let mut dst = vec![0_u8; dst_width * dst_height * 3];
    let src_stride = src_width * 4;

    for dy in 0..dst_height {
        let sy = (dy * src_height) / dst_height;
        let row_start = sy * src_stride;
        let dst_row_start = dy * dst_width * 3;

        for dx in 0..dst_width {
            let sx = (dx * src_width) / dst_width;
            let src_idx = row_start + sx * 4;

            if src_idx + 3 < src.len() {
                let out_idx = dst_row_start + dx * 3;
                if is_bgra {
                    dst[out_idx] = src[src_idx + 2]; // R
                    dst[out_idx + 1] = src[src_idx + 1]; // G
                    dst[out_idx + 2] = src[src_idx]; // B
                } else {
                    dst[out_idx] = src[src_idx]; // R
                    dst[out_idx + 1] = src[src_idx + 1]; // G
                    dst[out_idx + 2] = src[src_idx + 2]; // B
                }
            }
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuv_to_rgb_converts_neutral_gray() {
        let (r, g, b) = yuv_to_rgb(128, 128, 128);
        assert!((i32::from(r) - 130).abs() <= 5);
        assert!((i32::from(g) - 130).abs() <= 5);
        assert!((i32::from(b) - 130).abs() <= 5);
    }

    #[test]
    fn convert_yuy2_produces_correct_output_size() {
        let src = vec![128_u8; 640 * 480 * 2];
        let rgb = convert_yuy2_to_rgb24(&src, 640, 480, 320, 240);
        assert_eq!(rgb.len(), 320 * 240 * 3);
    }

    #[test]
    fn convert_nv12_produces_correct_output_size() {
        let src = vec![128_u8; 640 * 480 + (640 * 480 / 2)];
        let rgb = convert_nv12_to_rgb24(&src, 640, 480, 320, 240);
        assert_eq!(rgb.len(), 320 * 240 * 3);
    }

    #[test]
    fn convert_rgb32_produces_correct_output_size() {
        let src = vec![255_u8; 640 * 480 * 4];
        let rgb = convert_rgb32_to_rgb24(&src, 640, 480, 320, 240, true);
        assert_eq!(rgb.len(), 320 * 240 * 3);
    }

    #[test]
    fn downsample_rgb24_produces_correct_output_size() {
        let src = vec![255_u8; 640 * 480 * 3];
        let rgb = downsample_rgb24(&src, 640, 480, 320, 240);
        assert_eq!(rgb.len(), 320 * 240 * 3);
    }
}
