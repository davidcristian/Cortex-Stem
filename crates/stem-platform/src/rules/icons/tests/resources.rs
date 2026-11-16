use pretty_assertions::assert_eq;

use super::*;

#[test]
fn pe_image_paths_are_exe_dll_scr_in_any_case() {
    for path in [
        r"C:\Program Files\Discord\Discord.exe",
        r"C:\WINDOWS\EXPLORER.EXE",
        r"C:\Windows\System32\shell32.dll",
        "C:/Windows/System32/imageres.Dll",
        r"C:\Windows\System32\Bubbles.scr",
        "app.exe",
    ] {
        assert!(is_pe_image_path(path), "{path}");
    }
    for path in [
        r"C:\Users\me\Desktop\Discord.lnk",
        r"C:\dir.exe\readme.txt",
        r"C:\Program Files\App\app.exe\",
        r"C:\Program Files\App\.exe",
        "exe",
        "app.exe.bak",
        "notes.",
        "",
    ] {
        assert!(!is_pe_image_path(path), "{path}");
    }
}

/// A `GRPICONDIR` with the given entries (`width, height, bit_count,
/// bytes_in_res, id`), the type word as given.
fn group_dir(kind: u16, entries: &[(u8, u8, u16, u32, u16)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(w, h, bits, size, id) in entries {
        bytes.extend_from_slice(&[w, h, 0, 0]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes
}

fn entry(width: u32, height: u32, bit_count: u16, bytes_in_res: u32, id: u16) -> GroupIconEntry {
    GroupIconEntry {
        width,
        height,
        bit_count,
        bytes_in_res,
        id,
    }
}

#[test]
fn parse_group_icon_dir_reads_entries_and_maps_zero_to_256() {
    let bytes = group_dir(
        1,
        &[
            (16, 16, 4, 744, 1),
            (48, 48, 32, 9640, 2),
            (0, 0, 32, 9442, 3),
        ],
    );
    assert_eq!(
        bytes.len(),
        GROUP_ICON_HEADER_LEN + 3 * GROUP_ICON_ENTRY_LEN
    );
    assert_eq!(
        parse_group_icon_dir(&bytes),
        Ok(vec![
            entry(16, 16, 4, 744, 1),
            entry(48, 48, 32, 9640, 2),
            entry(256, 256, 32, 9442, 3),
        ])
    );
    // Trailing bytes (padding) are tolerated; junk in idReserved too.
    let mut padded = bytes.clone();
    padded.extend_from_slice(&[0xAA, 0xBB]);
    padded[0] = 0xFF;
    assert_eq!(parse_group_icon_dir(&padded).map(|e| e.len()), Ok(3));
    // A non-square entry keeps both sides.
    let wide = group_dir(1, &[(64, 0, 24, 100, 7)]);
    assert_eq!(
        parse_group_icon_dir(&wide),
        Ok(vec![entry(64, 256, 24, 100, 7)])
    );
}

#[test]
fn parse_group_icon_dir_rejects_malformed_directories() {
    assert_eq!(
        parse_group_icon_dir(&[]),
        Err(GroupIconError::Truncated { len: 0, needed: 6 })
    );
    assert_eq!(
        parse_group_icon_dir(&[0, 0, 1, 0, 1]),
        Err(GroupIconError::Truncated { len: 5, needed: 6 })
    );
    // Header says two entries, only one is present.
    let mut short = group_dir(1, &[(32, 32, 32, 4264, 1)]);
    short[4] = 2;
    assert_eq!(
        parse_group_icon_dir(&short),
        Err(GroupIconError::Truncated {
            len: 20,
            needed: 34
        })
    );
    // A cursor directory is not an icon directory.
    assert_eq!(
        parse_group_icon_dir(&group_dir(2, &[(32, 32, 1, 326, 1)])),
        Err(GroupIconError::BadType(2))
    );
    assert_eq!(
        parse_group_icon_dir(&group_dir(0, &[(32, 32, 1, 326, 1)])),
        Err(GroupIconError::BadType(0))
    );
    assert_eq!(
        parse_group_icon_dir(&group_dir(1, &[])),
        Err(GroupIconError::Empty)
    );
}

#[test]
fn select_group_icon_entry_prefers_largest_edge_then_depth_then_bytes() {
    // Discord-like directory: a 256 px PNG entry wins at 256.
    let entries = [
        entry(16, 16, 32, 1128, 1),
        entry(32, 32, 32, 4264, 2),
        entry(48, 48, 32, 9640, 3),
        entry(256, 256, 32, 9442, 4),
    ];
    assert_eq!(select_group_icon_entry(&entries, 256), Some(entries[3]));
    // The cap filters by edge: 48 for a 96 px request, 32 for 32.
    assert_eq!(select_group_icon_entry(&entries, 96), Some(entries[2]));
    assert_eq!(select_group_icon_entry(&entries, 32), Some(entries[1]));
    assert_eq!(select_group_icon_entry(&entries, 8), None);
    assert_eq!(select_group_icon_entry(&[], 256), None);
    // Same edge: the highest bit depth wins regardless of order.
    let depths = [
        entry(48, 48, 32, 9640, 1),
        entry(48, 48, 8, 3752, 2),
        entry(48, 48, 4, 1640, 3),
    ];
    assert_eq!(select_group_icon_entry(&depths, 256), Some(depths[0]));
    let mut reversed = depths;
    reversed.reverse();
    assert_eq!(select_group_icon_entry(&reversed, 256), Some(depths[0]));
    // Same edge and depth: the larger resource wins.
    let sizes = [
        entry(256, 256, 32, 9442, 1),
        entry(256, 256, 32, 270_376, 2),
    ];
    assert_eq!(select_group_icon_entry(&sizes, 256), Some(sizes[1]));
    // A full tie keeps the earliest entry.
    let ties = [entry(256, 256, 32, 9442, 1), entry(256, 256, 32, 9442, 2)];
    assert_eq!(select_group_icon_entry(&ties, 256), Some(ties[0]));
    // The longer side counts as the edge.
    let oblong = [entry(64, 128, 32, 100, 1), entry(96, 96, 32, 100, 2)];
    assert_eq!(select_group_icon_entry(&oblong, 256), Some(oblong[0]));
    assert_eq!(select_group_icon_entry(&oblong, 100), Some(oblong[1]));
}

#[test]
fn group_icon_errors_describe_themselves() {
    assert_eq!(
        GroupIconError::Truncated { len: 5, needed: 6 }.to_string(),
        "icon group directory is truncated (5 bytes, 6 needed)"
    );
    assert_eq!(
        GroupIconError::BadType(2).to_string(),
        "icon group directory has type 2, expected 1"
    );
}
