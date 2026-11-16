mod image;
mod pixels;
mod resources;
mod scale;

use pretty_assertions::assert_eq;

use super::*;

const BLACK: [u8; 4] = [0, 0, 0, 0];
const WHITE: [u8; 4] = [255, 255, 255, 0];

fn px(bgra: [u8; 4]) -> Vec<u8> {
    bgra.to_vec()
}

fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("buffer size")];
    let info = reader.next_frame(&mut buf).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

#[test]
fn only_window_icon_keys_are_left_out_of_the_cache() {
    let cacheable = |key: &str| is_cacheable(&IconKey(key.to_owned()));
    assert!(!cacheable("hwnd:132456:4242"));
    assert!(!cacheable("hwnd:132456"));
    assert!(cacheable(r"exe:C:\Windows\explorer.exe"));
    assert!(cacheable(r"file:C:\Users\me\notes.txt"));
    assert!(cacheable(
        "aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
    ));
    // The namespace is compared whole, and a key without one (which
    // loads nothing) is not left out.
    assert!(cacheable("hwndx:1"));
    assert!(cacheable("hwnd"));
    // The same rule as a hidden app's saved icon key.
    for key in ["hwnd:1:2", "hwnd:1", "hwndx:1", "hwnd", "exe:a", ""] {
        let key = IconKey(key.to_owned());
        assert_eq!(is_cacheable(&key), !is_window_icon_key(&key), "{key:?}");
    }
}
