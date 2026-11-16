use pretty_assertions::assert_eq;

use super::*;

#[test]
fn convert_icon_with_alpha_ignores_mask_and_keeps_straight_alpha() {
    // BGRA: blue-ish pixel at half alpha; white mask would make it transparent.
    let color = px([200, 100, 50, 128]);
    let png = convert_icon(1, 1, &color, Some(&WHITE), AlphaMode::Straight).expect("png");
    assert_eq!(decode(&png).2, vec![50, 100, 200, 128]);
}

#[test]
fn convert_icon_premultiplied_is_unpremultiplied() {
    let color = px([0, 32, 64, 128]);
    let png = convert_icon(1, 1, &color, None, AlphaMode::Premultiplied).expect("png");
    assert_eq!(decode(&png).2, vec![128, 64, 0, 128]);
}

#[test]
fn convert_icon_without_alpha_uses_mask() {
    let color = [[1, 2, 3, 0], [4, 5, 6, 0]].concat();
    let mask = [WHITE, BLACK].concat();
    let png = convert_icon(2, 1, &color, Some(&mask), AlphaMode::Straight).expect("png");
    assert_eq!(decode(&png).2, vec![3, 2, 1, 0, 6, 5, 4, 255]);
}

#[test]
fn convert_icon_without_alpha_or_mask_is_opaque() {
    let color = px([1, 2, 3, 0]);
    let png = convert_icon(1, 1, &color, None, AlphaMode::Straight).expect("png");
    assert_eq!(decode(&png).2, vec![3, 2, 1, 255]);
    // A mask of the wrong size is ignored, not misapplied.
    let png = convert_icon(1, 1, &color, Some(&[0; 8]), AlphaMode::Straight).expect("png");
    assert_eq!(decode(&png).2, vec![3, 2, 1, 255]);
}

#[test]
fn convert_icon_rejects_bad_buffers() {
    assert_eq!(
        convert_icon(2, 2, &[0; 4], None, AlphaMode::Straight),
        Err(IconConvertError::BufferSize {
            expected: 16,
            actual: 4
        })
    );
    assert_eq!(
        convert_icon(0, 0, &[], None, AlphaMode::Straight),
        Err(IconConvertError::BadDimensions {
            width: 0,
            height: 0
        })
    );
}

/// A 32x32 BGRA icon with a transparent frame and an opaque red centre.
fn framed_red_32() -> Vec<u8> {
    let (w, h) = (32u32, 32u32);
    let mut color = Vec::with_capacity(buffer_len(w, h).unwrap());
    for y in 0..h {
        for x in 0..w {
            let inside = (4..28).contains(&x) && (4..28).contains(&y);
            color.extend_from_slice(&[0, 0, 255, if inside { 255 } else { 0 }]);
        }
    }
    color
}

#[test]
fn convert_icon_handles_a_realistic_32px_buffer() {
    let (w, h) = (32u32, 32u32);
    let color = framed_red_32();
    let png = convert_icon(w, h, &color, None, AlphaMode::Straight).expect("png");
    let (dw, dh, rgba) = decode(&png);
    assert_eq!((dw, dh), (w, h));
    // Straight alpha: a transparent pixel keeps its colour bytes.
    assert_eq!(&rgba[..4], &[255, 0, 0, 0]);
    let centre = ((16 * w + 16) as usize) * BYTES_PER_PIXEL;
    assert_eq!(&rgba[centre..centre + 4], &[255, 0, 0, 255]);
}

#[test]
fn icon_image_validates_its_buffer() {
    assert_eq!(
        IconImage::new(2, 1, vec![0; 4]),
        Err(IconConvertError::BufferSize {
            expected: 8,
            actual: 4
        })
    );
    assert_eq!(
        IconImage::new(0, 1, vec![]),
        Err(IconConvertError::BadDimensions {
            width: 0,
            height: 1
        })
    );
    let image = IconImage::new(2, 1, vec![0; 8]).expect("image");
    assert_eq!(image.edge(), 2);
    assert_eq!(IconImage::new(3, 7, vec![0; 84]).unwrap().edge(), 7);
}

#[test]
fn fit_within_never_upscales_and_keeps_the_aspect_ratio() {
    let small = IconImage::new(32, 32, vec![7; 32 * 32 * 4]).unwrap();
    assert_eq!(small.clone().fit_within(48), small);
    assert_eq!(small.clone().fit_within(32), small);
    let wide = IconImage::new(64, 32, vec![7; 64 * 32 * 4]).unwrap();
    let fitted = wide.fit_within(16);
    assert_eq!((fitted.width, fitted.height), (16, 8));
    assert_eq!(fitted.rgba.len(), 16 * 8 * 4);
    assert!(fitted.rgba.iter().all(|&b| b == 7));
}

#[test]
fn decode_fit_encode_pipeline_shrinks_the_realistic_icon() {
    let image = decode_icon(32, 32, &framed_red_32(), None, AlphaMode::Straight).unwrap();
    assert_eq!((image.width, image.height), (32, 32));
    let fitted = image.fit_within(16);
    let png = fitted.encode_png().expect("png");
    let (w, h, rgba) = decode(&png);
    assert_eq!((w, h), (16, 16));
    // The 4-pixel transparent frame became a 2-pixel one: corner is
    // transparent (and zeroed), the pixel at (2, 2) is fully opaque red,
    // and nothing in between is fringed with another colour.
    assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
    let at = |x: usize, y: usize| &rgba[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
    assert_eq!(at(2, 2), &[255, 0, 0, 255]);
    assert_eq!(at(8, 8), &[255, 0, 0, 255]);
    assert_eq!(at(1, 8), &[0, 0, 0, 0]);
    for px in rgba.chunks_exact(4) {
        assert!(px[3] == 0 || px[..3] == [255, 0, 0], "pixel {px:?}");
    }
}
