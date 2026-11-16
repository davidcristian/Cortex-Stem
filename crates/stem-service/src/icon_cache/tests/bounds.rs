//! The bounds: eviction by count and by size, the scan at start and the
//! recency a hit saves.

use pretty_assertions::assert_eq;

use super::*;

fn set_age(path: &Path, seconds_ago: u64) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
        .unwrap();
}

#[test]
fn the_file_bound_evicts_the_least_recently_used_entry() {
    let root = TempDir::new("files");
    let limits = CacheLimits {
        max_bytes: DEFAULT_MAX_BYTES,
        max_files: 3,
    };
    let keys: Vec<IconKey> = (0..4)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    {
        let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
        for (i, key) in keys.iter().take(3).enumerate() {
            cache
                .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
                .unwrap();
            cache.flush();
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // The next start uses app0 again (a disk hit), so app1 is now the
    // least recently used.
    let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
    cache.get_or_load(&keys[0], 48, || Ok(None)).unwrap();
    cache.flush();
    std::thread::sleep(Duration::from_millis(5));
    cache
        .get_or_load(&keys[3], 48, || Ok(Some(png(3, 200))))
        .unwrap();
    cache.flush();

    let stats = cache.stats();
    assert_eq!((stats.hits, stats.entries, stats.evictions), (1, 3, 1));
    let mut expected: Vec<String> = [0, 2, 3]
        .iter()
        .map(|&i| entry_name(&keys[i], 48))
        .collect();
    expected.sort();
    assert_eq!(entries(&cache_dir(&root)), expected);
}

#[test]
fn the_byte_bound_evicts_oldest_first_and_never_the_entry_just_written() {
    let root = TempDir::new("bytes");
    let one = u64::try_from(
        encode(
            &build_id().unwrap(),
            "exe:",
            48,
            Stamp {
                len: 0,
                modified_ns: 0,
            },
            &png(0, 1000),
        )
        .len(),
    )
    .unwrap();
    let keys: Vec<IconKey> = (0..5)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    // Room for two and a half entries (keys differ in length by nothing).
    let key_len = u64::try_from(keys[0].0.len() - "exe:".len()).unwrap();
    let limits = CacheLimits {
        max_bytes: (one + key_len) * 5 / 2,
        max_files: DEFAULT_MAX_FILES,
    };
    let cache = DiskIconCache::open_with_limits(cache_dir(&root), limits).unwrap();
    for (i, key) in keys.iter().enumerate() {
        cache
            .get_or_load(key, 48, || Ok(Some(png(i as u8, 1000))))
            .unwrap();
        cache.flush();
        std::thread::sleep(Duration::from_millis(5));
    }
    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.evictions, 3);
    assert!(stats.bytes <= limits.max_bytes, "{stats:?}");
    let mut expected: Vec<String> = [3, 4].iter().map(|&i| entry_name(&keys[i], 48)).collect();
    expected.sort();
    assert_eq!(entries(&cache_dir(&root)), expected);

    // One entry larger than the whole bound is kept (it was just
    // written) and evicts everything else.
    let tiny = CacheLimits {
        max_bytes: 10,
        max_files: DEFAULT_MAX_FILES,
    };
    drop(cache);
    let cache = DiskIconCache::open_with_limits(cache_dir(&root), tiny).unwrap();
    cache.flush();
    assert_eq!(
        cache.stats().entries,
        0,
        "the start evicts down to the bound"
    );
    cache
        .get_or_load(&keys[0], 48, || Ok(Some(png(9, 1000))))
        .unwrap();
    cache.flush();
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn a_start_indexes_by_recency_evicts_to_the_bounds_and_removes_stray_temp_files() {
    let root = TempDir::new("scan");
    let keys: Vec<IconKey> = (0..3)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    {
        let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
        for (i, key) in keys.iter().enumerate() {
            cache
                .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
                .unwrap();
        }
        cache.flush();
    }
    let dir = cache_dir(&root);
    // app1 was used most recently, app0 least.
    set_age(&dir.join(entry_name(&keys[0], 48)), 3000);
    set_age(&dir.join(entry_name(&keys[1], 48)), 10);
    set_age(&dir.join(entry_name(&keys[2], 48)), 2000);
    let stray = dir.join(temp_name(&entry_name(&keys[0], 48), 7));
    fs::write(&stray, b"half an entry").unwrap();
    let foreign = dir.join("notes.txt");
    fs::write(&foreign, b"not ours").unwrap();

    let limits = CacheLimits {
        max_bytes: DEFAULT_MAX_BYTES,
        max_files: 2,
    };
    let cache = DiskIconCache::open_with_limits(&dir, limits).unwrap();
    cache.flush();
    assert!(!stray.exists());
    assert!(
        foreign.exists(),
        "files that are not entries are left alone"
    );
    let mut expected: Vec<String> = [1, 2].iter().map(|&i| entry_name(&keys[i], 48)).collect();
    expected.sort();
    assert_eq!(entries(&dir), expected);
    assert_eq!(cache.stats().evictions, 1);
}

#[test]
fn a_hit_on_an_old_entry_saves_its_recency_for_the_next_start() {
    let root = TempDir::new("touch");
    let key = root.exe("app.exe", b"MZ");
    let dir = cache_dir(&root);
    {
        let cache = DiskIconCache::open(&dir).unwrap();
        cache
            .get_or_load(&key, 48, || Ok(Some(png(1, 200))))
            .unwrap();
    }
    let file = dir.join(entry_name(&key, 48));
    let old = TOUCH_PERSIST_AFTER.as_secs() + 60;
    set_age(&file, old);
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    let cache = DiskIconCache::open(&dir).unwrap();
    assert!(cache.get_or_load(&key, 48, || Ok(None)).unwrap().is_some());
    cache.flush();
    let after = fs::metadata(&file).unwrap().modified().unwrap();
    assert!(
        after.duration_since(before).unwrap() >= Duration::from_secs(old - 5),
        "{before:?} -> {after:?}"
    );
    // A recent entry is not rewritten on a hit (the next start's).
    drop(cache);
    let cache = DiskIconCache::open(&dir).unwrap();
    assert!(cache.get_or_load(&key, 48, || Ok(None)).unwrap().is_some());
    cache.flush();
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), after);
}
