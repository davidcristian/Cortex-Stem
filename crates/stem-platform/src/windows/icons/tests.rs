use super::*;

fn system_file(relative: &str) -> String {
    let root = std::env::var("SystemRoot").expect("SystemRoot is set on Windows");
    format!("{root}\\{relative}")
}

fn decode(png: &[u8]) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let reader = decoder.read_info().expect("png header");
    (reader.info().width, reader.info().height)
}

#[test]
fn pe_resources_yield_the_native_image_of_system_binaries() {
    for relative in ["explorer.exe", "System32\\shell32.dll"] {
        let path = system_file(relative);
        let image = pe_resource_icon(&path)
            .unwrap_or_else(|e| panic!("{relative}: {e}"))
            .unwrap_or_else(|| panic!("{relative}: no icon group"));
        assert_eq!(image.width, image.height, "{relative}");
        assert!(
            (16..=256).contains(&image.edge()),
            "{relative}: {}x{}",
            image.width,
            image.height
        );
        let alphas: Vec<u8> = image.rgba.chunks_exact(4).map(|px| px[3]).collect();
        assert!(alphas.contains(&255), "{relative}: no opaque pixel");
        assert!(alphas.contains(&0), "{relative}: no transparent pixel");
    }
}

#[test]
fn pe_resources_win_the_file_chain_and_the_downscale_happens_here() {
    let path = system_file("explorer.exe");
    let loaded = load_file_icon(&path, 48).expect("load").expect("icon");
    assert_eq!(loaded.chain, "PE resources");
    assert!(loaded.image.edge() >= 48, "source {}", loaded.image.edge());
    let fitted = loaded.image.fit_within(48);
    assert_eq!((fitted.width, fitted.height), (48, 48));
}

#[test]
fn exe_key_png_is_exactly_the_requested_size() {
    let key = IconKey(format!("exe:{}", system_file("explorer.exe")));
    for size in [32, 48, 96, 192] {
        let png = load_icon_png(&key, size).expect("load").expect("png");
        assert_eq!(decode(&png), (size, size));
    }
}

#[test]
fn pe_resources_reject_what_is_not_a_pe_image() {
    assert!(pe_resource_icon(&system_file("win.ini")).is_err());
    assert!(pe_resource_icon(&system_file("does-not-exist.exe")).is_err());
}
