//! The mock's icons: a rounded square in a colour that depends only on the
//! key.

/// Deterministic colour per key so screenshots are stable.
pub(super) fn color_for(key: &str) -> [u8; 4] {
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let hue = (hash % 360) as f32;
    let (r, g, b) = hsl_to_rgb(hue, 0.65, 0.55);
    [r, g, b, 255]
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// A rounded square PNG in one colour.
pub fn solid_png(size: u32, rgba: [u8; 4]) -> Vec<u8> {
    let radius = (size as f32) * 0.22;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let inside = rounded_rect_contains(x as f32 + 0.5, y as f32 + 0.5, size as f32, radius);
            data.extend_from_slice(&[rgba[0], rgba[1], rgba[2], if inside { rgba[3] } else { 0 }]);
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .expect("png header for in-memory buffer");
        writer
            .write_image_data(&data)
            .expect("png data for in-memory buffer");
    }
    out
}

fn rounded_rect_contains(x: f32, y: f32, size: f32, r: f32) -> bool {
    let cx = x.clamp(r, size - r);
    let cy = y.clamp(r, size - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}
