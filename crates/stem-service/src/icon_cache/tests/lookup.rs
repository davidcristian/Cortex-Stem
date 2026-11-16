//! Reads: hits, misses, and what a session writes.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_cacheable_icon_is_loaded_once_and_read_from_disk_after_a_restart() {
    let root = TempDir::new("restart");
    let key = root.exe("app.exe", b"MZ version 1");
    let mock = MockPlatform::new();
    let loader = Loader::new();
    let first = {
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let got = cache
            .get_or_load(&key, 96, || loader.answer(mock.icon_png(&key, 96)))
            .unwrap()
            .unwrap();
        cache.flush();
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().writes, 1);
        assert_eq!(cache.stats().entries, 1);
        got
    };
    assert_eq!(loader.calls(), 1);
    assert_eq!(entries(&cache_dir(&root)).len(), 1);

    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    for _ in 0..3 {
        let got = cache
            .get_or_load(&key, 96, || loader.answer(mock.icon_png(&key, 96)))
            .unwrap();
        assert_eq!(got.as_deref(), Some(first.as_slice()));
    }
    assert_eq!(loader.calls(), 1, "a hit never asks the platform");
    // Another size is another entry.
    cache
        .get_or_load(&key, 48, || loader.answer(mock.icon_png(&key, 48)))
        .unwrap();
    assert_eq!(loader.calls(), 2);
    cache.flush();
    let stats = cache.stats();
    // One disk hit; the two requests after it come from memory.
    assert_eq!(
        (stats.hits, stats.repeats, stats.misses, stats.writes),
        (1, 2, 1, 1)
    );
    assert_eq!(stats.entries, 2);
    assert_eq!(entries(&cache_dir(&root)).len(), 2, "no temp file left");
}

#[test]
fn a_changed_exe_misses_whether_its_size_or_only_its_time_changed() {
    let root = TempDir::new("changed");
    let key = root.exe("app.exe", b"MZ version 1");
    let path = source_file(&key).unwrap().to_path_buf();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let loader = Loader::new();
    let load = |tag| loader.answer(Ok(Some(png(tag, 100))));

    assert_eq!(
        cache.get_or_load(&key, 48, || load(1)).unwrap(),
        Some(png(1, 100))
    );
    cache.flush();
    drop(cache);

    // A new build of the exe (another length) misses after a restart
    // and its icon replaces the old entry.
    fs::write(&path, b"MZ version 2, longer").unwrap();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    assert_eq!(
        cache.get_or_load(&key, 48, || load(2)).unwrap(),
        Some(png(2, 100))
    );
    assert_eq!(loader.calls(), 2);
    cache.flush();
    drop(cache);

    // Same length, other contents and time: still a miss.
    fs::write(&path, b"MZ version 3, longer").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    assert_eq!(
        cache.get_or_load(&key, 48, || load(3)).unwrap(),
        Some(png(3, 100))
    );
    assert_eq!(loader.calls(), 3);
    cache.flush();
    drop(cache);

    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    assert_eq!(
        cache.get_or_load(&key, 48, || load(9)).unwrap(),
        Some(png(3, 100))
    );
    assert_eq!(loader.calls(), 3);
    assert_eq!(entries(&cache_dir(&root)).len(), 1);
}

#[test]
fn a_file_that_changes_during_the_session_is_not_written_with_the_old_bitmap() {
    let root = TempDir::new("session");
    let key = root.exe("app.exe", b"MZ version 1");
    let path = source_file(&key).unwrap().to_path_buf();
    // Like the platform: the first bitmap it loads for a key is what
    // it returns for the rest of the session.
    let loader = Loader::new();
    let platform = || loader.answer(Ok(Some(png(1, 100))));

    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    cache.get_or_load(&key, 48, platform).unwrap();
    cache.flush();
    // The app updates itself while Stem runs.
    fs::write(&path, b"MZ version 2, longer").unwrap();
    assert_eq!(
        cache.get_or_load(&key, 48, platform).unwrap(),
        Some(png(1, 100))
    );
    cache.flush();
    assert_eq!(cache.stats().writes, 1, "the stale bitmap is not written");
    drop(cache);

    // The next start loads the new icon and keeps it.
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let fresh = || loader.answer(Ok(Some(png(2, 100))));
    assert_eq!(
        cache.get_or_load(&key, 48, fresh).unwrap(),
        Some(png(2, 100))
    );
    cache.flush();
    drop(cache);
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let calls = loader.calls();
    assert_eq!(
        cache.get_or_load(&key, 48, fresh).unwrap(),
        Some(png(2, 100))
    );
    assert_eq!(loader.calls(), calls);
}

#[test]
fn a_file_that_changes_during_its_load_is_never_written_this_session() {
    let root = TempDir::new("during");
    let key = root.exe("app.exe", b"MZ version 1");
    let path = source_file(&key).unwrap().to_path_buf();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let loader = Loader::new();
    cache
        .get_or_load(&key, 48, || {
            fs::write(&path, b"MZ version 2, longer").unwrap();
            loader.answer(Ok(Some(png(1, 100))))
        })
        .unwrap();
    // The platform now holds a bitmap of unknown vintage for the key.
    cache
        .get_or_load(&key, 48, || loader.answer(Ok(Some(png(1, 100)))))
        .unwrap();
    cache.flush();
    assert_eq!(cache.stats().writes, 0);
    assert_eq!(loader.calls(), 2);
    assert!(entries(&cache_dir(&root)).is_empty());
}

#[test]
fn later_requests_in_a_session_touch_no_file() {
    let root = TempDir::new("repeats");
    let hit = root.exe("hit.exe", b"MZ hit");
    let loaded = root.exe("loaded.exe", b"MZ loaded");
    let dir = cache_dir(&root);
    {
        let cache = DiskIconCache::open(&dir).unwrap();
        cache
            .get_or_load(&hit, 48, || Ok(Some(png(1, 100))))
            .unwrap();
        cache.flush();
    }
    let cache = DiskIconCache::open(&dir).unwrap();
    let loader = Loader::new();
    let from_disk = || loader.answer(Ok(None));
    let from_platform = || loader.answer(Ok(Some(png(2, 100))));
    assert_eq!(
        cache.get_or_load(&hit, 48, from_disk).unwrap(),
        Some(png(1, 100))
    );
    assert_eq!(
        cache.get_or_load(&loaded, 48, from_platform).unwrap(),
        Some(png(2, 100))
    );
    cache.flush();
    assert_eq!(loader.calls(), 1);

    // Without the source files and the cache directory the session
    // answers as before: a stamp would now fail (bypassed) and a read
    // miss. The hit comes from its bytes, the loaded key from the
    // platform (its memory cache, on Windows).
    fs::remove_file(source_file(&hit).unwrap()).unwrap();
    fs::remove_file(source_file(&loaded).unwrap()).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    for _ in 0..3 {
        assert_eq!(
            cache.get_or_load(&hit, 48, from_disk).unwrap(),
            Some(png(1, 100))
        );
        assert_eq!(
            cache.get_or_load(&loaded, 48, from_platform).unwrap(),
            Some(png(2, 100))
        );
    }
    assert_eq!(loader.calls(), 4, "the hit never reaches the platform");
    cache.flush();
    let stats = cache.stats();
    assert_eq!(
        (stats.hits, stats.misses, stats.repeats, stats.bypassed),
        (1, 1, 6, 0)
    );
    assert_eq!((stats.writes, stats.write_failures), (1, 0));
}

#[test]
fn of_two_racing_loads_only_the_first_is_written() {
    let root = TempDir::new("race");
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();

    // Another request for the same icon arrives while this one loads
    // and finishes first: one write, not two.
    let same = root.exe("same.exe", b"MZ");
    let got = cache
        .get_or_load(&same, 48, || {
            let inner = cache.get_or_load(&same, 48, || Ok(Some(png(1, 100))));
            assert_eq!(inner.unwrap(), Some(png(1, 100)));
            Ok(Some(png(1, 100)))
        })
        .unwrap();
    assert_eq!(got, Some(png(1, 100)));
    cache.flush();
    assert_eq!(cache.stats().writes, 1);

    // The app updates itself between the two: the racing request loads
    // the new file and is written; the first one's older bitmap is not.
    let updated = root.exe("updated.exe", b"MZ version 1");
    let path = source_file(&updated).unwrap().to_path_buf();
    let got = cache
        .get_or_load(&updated, 48, || {
            fs::write(&path, b"MZ version 2, longer").unwrap();
            let inner = cache.get_or_load(&updated, 48, || Ok(Some(png(2, 100))));
            assert_eq!(inner.unwrap(), Some(png(2, 100)));
            Ok(Some(png(1, 100)))
        })
        .unwrap();
    assert_eq!(got, Some(png(1, 100)));
    cache.flush();
    assert_eq!(cache.stats().writes, 2);
    drop(cache);
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    assert_eq!(
        cache.get_or_load(&updated, 48, || Ok(None)).unwrap(),
        Some(png(2, 100))
    );
}

#[test]
fn keys_that_change_on_their_own_are_never_cached() {
    let root = TempDir::new("uncached");
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let mock = MockPlatform::new();
    let loader = Loader::new();
    let lnk = root.path().join("app.lnk");
    fs::write(&lnk, b"shortcut").unwrap();
    let keys = [
        IconKey("hwnd:1234:56".into()),
        IconKey::shell("recycle-bin-full"),
        IconKey("aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
        IconKey("mock:exe:x".into()),
        IconKey(format!("file:{}", lnk.display())),
    ];
    for key in &keys {
        for _ in 0..2 {
            let expected = mock.icon_png(key, 48).unwrap();
            let got = cache
                .get_or_load(key, 48, || loader.answer(mock.icon_png(key, 48)))
                .unwrap();
            assert_eq!(got, expected);
        }
    }
    cache.flush();
    assert_eq!(loader.calls(), keys.len() * 2);
    assert_eq!(cache.stats().bypassed, 10);
    assert!(entries(&cache_dir(&root)).is_empty());
}

#[test]
fn errors_none_and_missing_files_pass_through_and_write_nothing() {
    let root = TempDir::new("errors");
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let key = root.exe("app.exe", b"MZ");
    let err = cache.get_or_load(&key, 48, || Err(PlatformError::Timeout));
    assert!(matches!(err, Err(PlatformError::Timeout)));
    assert_eq!(cache.get_or_load(&key, 48, || Ok(None)).unwrap(), None);
    let gone = IconKey(format!("exe:{}", root.path().join("gone.exe").display()));
    let loader = Loader::new();
    for _ in 0..2 {
        let got = cache
            .get_or_load(&gone, 48, || loader.answer(Ok(Some(png(1, 50)))))
            .unwrap();
        assert_eq!(got, Some(png(1, 50)));
    }
    assert_eq!(loader.calls(), 2);
    // Too large to cache: returned, not written.
    let huge = png(2, MAX_PAYLOAD_BYTES + 1);
    let other = root.exe("big.exe", b"MZ");
    assert_eq!(
        cache
            .get_or_load(&other, 48, || Ok(Some(huge.clone())))
            .unwrap(),
        Some(huge)
    );
    cache.flush();
    assert_eq!(cache.stats().writes, 0);
    assert!(entries(&cache_dir(&root)).is_empty());
}

#[test]
fn a_corrupt_or_foreign_entry_is_a_miss_and_is_replaced() {
    let root = TempDir::new("corrupt");
    let key = root.exe("app.exe", b"MZ");
    let name = entry_name(&key, 48);
    let file = cache_dir(&root).join(&name);
    let good = || -> Vec<u8> {
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        cache
            .get_or_load(&key, 48, || Ok(Some(png(1, 300))))
            .unwrap();
        cache.flush();
        fs::read(&file).unwrap()
    };
    let bytes = good();
    let stamp = Stamp::of(source_file(&key).unwrap()).unwrap();

    let mut flipped = bytes.clone();
    flipped[bytes.len() / 2] ^= 0xff;
    let build = build_id().unwrap();
    let other_key = encode(&build, "exe:C:/other.exe", 48, stamp, &png(5, 300));
    let other_size = encode(&build, &key.0, 96, stamp, &png(5, 300));
    let other_version = encode("0.0.0-old", &key.0, 48, stamp, &png(5, 300));
    // What the cache's first version wrote: the crate version alone.
    let version_only = encode(CRATE_VERSION, &key.0, 48, stamp, &png(5, 300));
    let other_build = encode(
        &format!("{CRATE_VERSION}+1.2"),
        &key.0,
        48,
        stamp,
        &png(5, 300),
    );
    let cases: [(&str, Vec<u8>, bool); 9] = [
        ("empty", Vec::new(), true),
        ("truncated", bytes[..bytes.len() - 3].to_vec(), true),
        ("flipped", flipped, true),
        ("garbage", vec![0xAB; 5000], true),
        ("another key's", other_key, false),
        ("another size's", other_size, false),
        ("another version's", other_version, false),
        ("a version-only producer's", version_only, false),
        ("another build's", other_build, false),
    ];
    for (what, contents, corrupt) in cases {
        fs::write(&file, &contents).unwrap();
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let loader = Loader::new();
        let got = cache
            .get_or_load(&key, 48, || loader.answer(Ok(Some(png(2, 300)))))
            .unwrap();
        assert_eq!(got, Some(png(2, 300)), "{what}");
        assert_eq!(loader.calls(), 1, "{what} entry must miss");
        cache.flush();
        let stats = cache.stats();
        assert_eq!(
            (stats.hits, stats.misses, stats.writes),
            (0, 1, 1),
            "{what}"
        );
        drop(cache);
        // Replaced by a valid entry for the key (a corrupt one was
        // deleted first).
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        let got = cache.get_or_load(&key, 48, || Ok(None)).unwrap();
        assert_eq!(
            got,
            Some(png(2, 300)),
            "{what} not replaced (corrupt: {corrupt})"
        );
    }

    // An entry the cache cannot open is a miss too: a directory where
    // the file should be.
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    let got = cache
        .get_or_load(&key, 48, || Ok(Some(png(3, 300))))
        .unwrap();
    assert_eq!(got, Some(png(3, 300)));
    cache.flush();
    assert_eq!(cache.stats().write_failures, 1);
}
