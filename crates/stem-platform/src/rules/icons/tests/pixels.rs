use pretty_assertions::assert_eq;

use super::*;

#[test]
fn buffer_len_rejects_degenerate_sizes() {
    assert_eq!(buffer_len(0, 4), None);
    assert_eq!(buffer_len(4, 0), None);
    assert_eq!(buffer_len(MAX_EDGE + 1, 1), None);
    assert_eq!(buffer_len(2, 3), Some(24));
}

#[test]
fn bgra_to_rgba_swaps_red_and_blue() {
    let bgra = [1, 2, 3, 4, 10, 20, 30, 40, 99];
    assert_eq!(bgra_to_rgba(&bgra), vec![3, 2, 1, 4, 30, 20, 10, 40]);
}

#[test]
fn has_any_alpha_detects_a_single_pixel() {
    assert!(!has_any_alpha(&[9, 9, 9, 0, 1, 1, 1, 0]));
    assert!(has_any_alpha(&[9, 9, 9, 0, 1, 1, 1, 7]));
}

#[test]
fn mask_alpha_white_is_transparent_black_is_opaque() {
    let mut rgba = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
    let mask = [WHITE, BLACK, [0, 0, 1, 0]].concat();
    apply_mask_alpha(&mut rgba, &mask);
    assert_eq!(rgba, [1, 2, 3, 0, 4, 5, 6, 255, 7, 8, 9, 0]);
}

#[test]
fn mask_alpha_without_counterpart_is_opaque() {
    let mut rgba = [1, 2, 3, 0, 4, 5, 6, 0];
    apply_mask_alpha(&mut rgba, &WHITE);
    assert_eq!(rgba, [1, 2, 3, 0, 4, 5, 6, 255]);
}

#[test]
fn unpremultiply_restores_straight_colour() {
    let mut rgba = [
        64, 32, 0, 128, 10, 20, 30, 0, 200, 100, 50, 255, 255, 1, 0, 1,
    ];
    unpremultiply(&mut rgba);
    assert_eq!(
        rgba,
        [
            128, 64, 0, 128, 0, 0, 0, 0, 200, 100, 50, 255, 255, 255, 0, 1
        ]
    );
}

#[test]
fn split_stacked_mask_halves_the_buffer() {
    let stacked = [WHITE, BLACK, BLACK, WHITE].concat();
    let (and, xor) = split_stacked_mask(&stacked, 2, 1).expect("split");
    assert_eq!(and, [WHITE, BLACK].concat());
    assert_eq!(xor, [BLACK, WHITE].concat());
    assert_eq!(split_stacked_mask(&stacked, 2, 2), None);
    assert_eq!(split_stacked_mask(&stacked[..8], 2, 1), None);
}

#[test]
fn encode_png_round_trips_pixels() {
    let rgba = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 9, 8, 7, 6];
    let png = encode_png(2, 2, &rgba).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(decode(&png), (2, 2, rgba.to_vec()));
}

#[test]
fn encode_png_validates_input() {
    assert_eq!(
        encode_png(0, 1, &[]),
        Err(IconConvertError::BadDimensions {
            width: 0,
            height: 1
        })
    );
    assert_eq!(
        encode_png(1, 1, &[1, 2, 3]),
        Err(IconConvertError::BufferSize {
            expected: 4,
            actual: 3
        })
    );
}
