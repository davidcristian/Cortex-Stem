//! The area-averaging downscale that shrinks an icon to the requested edge;
//! it never upscales.

use super::pixels::{BYTES_PER_PIXEL, IconConvertError, buffer_len};

/// Dimensions that fit `width`×`height` inside a `max_edge` square while
/// preserving the aspect ratio (the shorter side rounded to nearest, at
/// least 1). `None` when the image already fits, or when `max_edge` is 0:
/// this is a shrink-only rule.
pub fn fit_dimensions(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    if max_edge == 0 || width == 0 || height == 0 || width.max(height) <= max_edge {
        return None;
    }
    let shrink = |side: u32, longer: u32| -> u32 {
        let scaled =
            (u64::from(side) * u64::from(max_edge) + u64::from(longer) / 2) / u64::from(longer);
        (scaled as u32).max(1)
    };
    Some(if width >= height {
        (max_edge, shrink(height, width))
    } else {
        (shrink(width, height), max_edge)
    })
}

/// For every destination index, the source indices it covers with their
/// overlap, in units of `1/dst` source pixels; the overlaps of one
/// destination index sum to `src`. Exact integer arithmetic, so weights of a
/// non-integer ratio (e.g. 3 → 2: 2 + 1, 1 + 2) carry no rounding error.
pub(super) fn coverage(src: u32, dst: u32) -> Vec<Vec<(usize, u64)>> {
    let (src, dst) = (u64::from(src), u64::from(dst));
    (0..dst)
        .map(|i| {
            let start = i * src;
            let end = start + src;
            let first = start / dst;
            let last = end.div_ceil(dst).min(src);
            (first..last)
                .filter_map(|j| {
                    let overlap = end.min((j + 1) * dst).saturating_sub(start.max(j * dst));
                    (overlap > 0).then_some((j as usize, overlap))
                })
                .collect()
        })
        .collect()
}

/// Area-averaging (box filter) downscale of a straight-alpha RGBA buffer to
/// `dst_width`×`dst_height`, each at most the source side. Colour is
/// averaged **premultiplied** (so fully transparent neighbours contribute no
/// colour and edges get no light or dark fringe) and converted back to
/// straight alpha, rounding to nearest. Fully transparent output pixels are
/// all zero. Never upscales: a larger target is `BadDimensions`.
pub fn downscale_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>, IconConvertError> {
    let expected =
        buffer_len(width, height).ok_or(IconConvertError::BadDimensions { width, height })?;
    if rgba.len() != expected {
        return Err(IconConvertError::BufferSize {
            expected,
            actual: rgba.len(),
        });
    }
    let out_len = buffer_len(dst_width, dst_height).ok_or(IconConvertError::BadDimensions {
        width: dst_width,
        height: dst_height,
    })?;
    if dst_width > width || dst_height > height {
        return Err(IconConvertError::BadDimensions {
            width: dst_width,
            height: dst_height,
        });
    }
    let columns = coverage(width, dst_width);
    let rows = coverage(height, dst_height);
    // Every destination pixel's weights sum to this (see `coverage`).
    let total = u64::from(width) * u64::from(height);
    let round_div = |n: u64, d: u64| ((n + d / 2) / d).min(255) as u8;
    let stride = width as usize * BYTES_PER_PIXEL;

    let mut out = Vec::with_capacity(out_len);
    for ys in &rows {
        for xs in &columns {
            let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
            for &(sy, wy) in ys {
                let row = &rgba[sy * stride..(sy + 1) * stride];
                for &(sx, wx) in xs {
                    let px = &row[sx * BYTES_PER_PIXEL..(sx + 1) * BYTES_PER_PIXEL];
                    let w = wy * wx;
                    let alpha = u64::from(px[3]);
                    let wa = w * alpha;
                    r += wa * u64::from(px[0]);
                    g += wa * u64::from(px[1]);
                    b += wa * u64::from(px[2]);
                    a += wa;
                }
            }
            let out_alpha = round_div(a, total);
            if out_alpha == 0 || a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[
                    round_div(r, a),
                    round_div(g, a),
                    round_div(b, a),
                    out_alpha,
                ]);
            }
        }
    }
    Ok(out)
}
