use pretty_assertions::assert_eq;

use super::*;
use crate::rules::icons::scale::coverage;
#[test]
fn fit_dimensions_shrinks_only_and_keeps_the_aspect_ratio() {
    assert_eq!(fit_dimensions(256, 256, 48), Some((48, 48)));
    assert_eq!(fit_dimensions(256, 128, 48), Some((48, 24)));
    assert_eq!(fit_dimensions(128, 256, 48), Some((24, 48)));
    assert_eq!(fit_dimensions(64, 48, 32), Some((32, 24)));
    // Rounded to nearest, never below 1.
    assert_eq!(fit_dimensions(100, 30, 10), Some((10, 3)));
    assert_eq!(fit_dimensions(100, 1, 10), Some((10, 1)));
    assert_eq!(fit_dimensions(1, 100, 10), Some((1, 10)));
    // Already fits: untouched (no upscaling).
    assert_eq!(fit_dimensions(48, 48, 48), None);
    assert_eq!(fit_dimensions(32, 32, 48), None);
    assert_eq!(fit_dimensions(16, 32, 32), None);
    // Degenerate inputs.
    assert_eq!(fit_dimensions(0, 32, 16), None);
    assert_eq!(fit_dimensions(32, 32, 0), None);
}

#[test]
fn coverage_weights_are_exact_for_non_integer_ratios() {
    // 3 source pixels into 2: each destination pixel spans 1.5 source
    // pixels, in units of 1/2: [2, 1] and [1, 2].
    assert_eq!(
        coverage(3, 2),
        vec![vec![(0, 2), (1, 1)], vec![(1, 1), (2, 2)]]
    );
    // Integer ratio: whole pixels, weight = dst each.
    assert_eq!(
        coverage(4, 2),
        vec![vec![(0, 2), (1, 2)], vec![(2, 2), (3, 2)]]
    );
    // Identity.
    assert_eq!(coverage(2, 2), vec![vec![(0, 2)], vec![(1, 2)]]);
    // Every destination index's overlaps sum to `src`.
    for (src, dst) in [(256, 48), (97, 13), (5, 4), (1024, 1)] {
        for weights in coverage(src, dst) {
            assert_eq!(weights.iter().map(|w| w.1).sum::<u64>(), u64::from(src));
        }
    }
}

#[test]
fn downscale_averages_whole_pixels() {
    // 2x2 opaque: red, green / blue, white -> one pixel, the mean.
    let rgba = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ]
    .concat();
    let out = downscale_rgba(2, 2, &rgba, 1, 1).expect("downscale");
    assert_eq!(out, vec![128, 128, 128, 255]);
}

#[test]
fn downscale_handles_fractional_coverage() {
    // 3x1 opaque greys 0, 90, 180 -> 2x1: (0*2 + 90*1)/3 and (90*1 + 180*2)/3.
    let rgba = [[0, 0, 0, 255], [90, 90, 90, 255], [180, 180, 180, 255]].concat();
    let out = downscale_rgba(3, 1, &rgba, 2, 1).expect("downscale");
    assert_eq!(out, vec![30, 30, 30, 255, 150, 150, 150, 255]);
}

#[test]
fn downscale_averages_premultiplied_so_transparent_neighbours_add_no_colour() {
    // An opaque red pixel next to a fully transparent *green* pixel: the
    // result is pure red at half alpha, not a brownish fringe.
    let rgba = [[255, 0, 0, 255], [0, 255, 0, 0]].concat();
    let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
    assert_eq!(out, vec![255, 0, 0, 128]);
    // Half-transparent white next to opaque black: premultiplied mean is
    // (255*128 + 0) / (128 + 255) for the colour, (128 + 255) / 2 for alpha.
    let rgba = [[255, 255, 255, 128], [0, 0, 0, 255]].concat();
    let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
    assert_eq!(out, vec![85, 85, 85, 192]);
    // All transparent stays all zero (no colour leaks through).
    let rgba = [[9, 9, 9, 0], [200, 1, 2, 0]].concat();
    let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
    assert_eq!(out, vec![0, 0, 0, 0]);
}

#[test]
fn downscale_rejects_upscaling_and_bad_buffers() {
    assert_eq!(
        downscale_rgba(2, 2, &[0; 16], 3, 2),
        Err(IconConvertError::BadDimensions {
            width: 3,
            height: 2
        })
    );
    assert_eq!(
        downscale_rgba(2, 2, &[0; 16], 1, 0),
        Err(IconConvertError::BadDimensions {
            width: 1,
            height: 0
        })
    );
    assert_eq!(
        downscale_rgba(2, 2, &[0; 8], 1, 1),
        Err(IconConvertError::BufferSize {
            expected: 16,
            actual: 8
        })
    );
    // Same size is a copy.
    let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(downscale_rgba(2, 1, &rgba, 2, 1).unwrap(), rgba.to_vec());
}
#[test]
fn downscale_of_a_large_icon_is_bounded_and_exact_on_flat_colour() {
    // 256 -> 48 (the shell's SIIGBF_BIGGERSIZEOK case): a flat colour
    // survives untouched, and alpha of a half-transparent flat image too.
    let image = IconImage::new(256, 256, [10, 20, 30, 200].repeat(256 * 256)).unwrap();
    let fitted = image.fit_within(48);
    assert_eq!((fitted.width, fitted.height), (48, 48));
    assert!(
        fitted
            .rgba
            .chunks_exact(4)
            .all(|px| px == [10, 20, 30, 200])
    );
}
