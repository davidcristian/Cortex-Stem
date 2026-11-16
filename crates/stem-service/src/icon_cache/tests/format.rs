//! What is cached, the entry names and the entry format.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn source_files_are_absolute_pe_images_under_exe_and_file_only() {
    let root = TempDir::new("source");
    let exe = root.exe("app.exe", b"MZ");
    assert!(source_file(&exe).is_some());
    let dll = IconKey(format!("file:{}", root.path().join("x.DLL").display()));
    assert!(source_file(&dll).is_some());
    for uncached in [
        IconKey("hwnd:1234:56".into()),
        IconKey::shell("recycle-bin-full"),
        IconKey::shell("recycle-bin-empty"),
        IconKey("aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
        IconKey("mock:exe:x".into()),
        IconKey(format!("file:{}", root.path().join("app.lnk").display())),
        IconKey(format!("file:{}", root.path().join("notes.txt").display())),
        IconKey("exe:relative/app.exe".into()),
        IconKey(r"exe:\\server\share\app.exe".into()),
        IconKey(r"file:\\?\UNC\server\share\app.exe".into()),
        IconKey("exe://server/share/app.exe".into()),
        IconKey("exe:".into()),
        IconKey("nonsense".into()),
        IconKey(format!(
            "exe:{}{}.exe",
            root.path().display(),
            "a".repeat(MAX_KEY_BYTES)
        )),
    ] {
        assert_eq!(source_file(&uncached), None, "{uncached:?}");
    }
}

#[test]
fn entries_round_trip_and_any_changed_byte_fails_to_decode() {
    let stamp = Stamp {
        len: 12345,
        modified_ns: 1_700_000_000_123_456_789,
    };
    let payload = png(7, 61);
    let build = "0.1.0+123456.1700000000000000000";
    let bytes = encode(build, "exe:C:/x.exe", 96, stamp, &payload);
    assert_eq!(
        decode(&bytes),
        Some(Decoded {
            producer: build.as_bytes(),
            key: b"exe:C:/x.exe",
            size: 96,
            stamp,
            png: &payload,
        })
    );
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0x10;
        assert_eq!(decode(&bad), None, "flipped byte {i}");
    }
    for len in 0..bytes.len() {
        assert_eq!(decode(&bytes[..len]), None, "truncated to {len}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(decode(&longer), None);
    // A payload that is not a PNG is refused even with a good checksum.
    assert_eq!(decode(&encode(build, "k", 1, stamp, b"GIF89a..")), None);
}

#[test]
fn entries_carry_the_running_build_and_another_build_misses() {
    let id = build_id().unwrap();
    let exe = Stamp::of(&std::env::current_exe().unwrap()).unwrap();
    assert_eq!(
        id,
        format!("{CRATE_VERSION}+{}.{}", exe.len, exe.modified_ns)
    );

    let root = TempDir::new("build");
    let key = root.exe("app.exe", b"MZ");
    let dir = cache_dir(&root);
    {
        let cache = DiskIconCache::open(&dir).unwrap();
        cache
            .get_or_load(&key, 48, || Ok(Some(png(1, 100))))
            .unwrap();
        cache.flush();
    }
    let bytes = fs::read(dir.join(entry_name(&key, 48))).unwrap();
    assert_eq!(decode(&bytes).unwrap().producer, id.as_bytes());

    // Another stem.exe of the same version (rebuilt, installed,
    // updated) misses and writes its own bitmap.
    let loader = Loader::new();
    {
        let other = DiskIconCache::open_as(
            dir.clone(),
            CacheLimits::default(),
            format!("{CRATE_VERSION}+1.2"),
        )
        .unwrap();
        let got = other
            .get_or_load(&key, 48, || loader.answer(Ok(Some(png(2, 100)))))
            .unwrap();
        assert_eq!(got, Some(png(2, 100)));
        other.flush();
    }
    assert_eq!(loader.calls(), 1);
    // And the running build misses that one in turn.
    let cache = DiskIconCache::open(&dir).unwrap();
    let got = cache
        .get_or_load(&key, 48, || loader.answer(Ok(Some(png(3, 100)))))
        .unwrap();
    assert_eq!(got, Some(png(3, 100)));
    assert_eq!(loader.calls(), 2);
}

#[test]
fn names_are_stable_hex_and_differ_by_key_and_size() {
    let a = entry_name(&IconKey("exe:C:/a.exe".into()), 48);
    assert!(is_entry_name(&a), "{a}");
    assert_ne!(a, entry_name(&IconKey("exe:C:/a.exe".into()), 96));
    assert_ne!(a, entry_name(&IconKey("exe:C:/b.exe".into()), 48));
    // Fixed across builds: a restart must find what the last run wrote.
    assert_eq!(
        entry_name(&IconKey("exe:C:/a.exe".into()), 48),
        entry_name(&IconKey("exe:C:/a.exe".into()), 48)
    );
    let temp = temp_name(&a, 3);
    assert!(is_temp_name(&temp), "{temp}");
    assert!(!is_entry_name(&temp));
    assert!(!is_temp_name(&a));
    assert!(!is_entry_name("notes.txt"));
    assert!(!is_entry_name("0123456789ABCDEF.icon"));
}
