//! A cache directory that goes away, a failed open, and shutdown.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_cache_directory_that_goes_away_is_recreated_by_the_next_write() {
    let root = TempDir::new("gone");
    let dir = cache_dir(&root);
    let cache = DiskIconCache::open(&dir).unwrap();
    // Once the writer's start-up scan is done with the directory.
    cache.flush();
    fs::remove_dir_all(&dir).unwrap();
    let key = root.exe("app.exe", b"MZ");
    cache
        .get_or_load(&key, 48, || Ok(Some(png(1, 200))))
        .unwrap();
    cache.flush();
    assert_eq!(cache.stats().writes, 1);
    assert_eq!(entries(&dir).len(), 1);
}

#[test]
fn open_fails_where_no_directory_can_be() {
    let root = TempDir::new("open");
    let file = root.path().join("a-file");
    fs::write(&file, b"x").unwrap();
    assert!(DiskIconCache::open(&file).is_err());
    assert!(DiskIconCache::open(file.join("below")).is_err());
}

#[test]
fn shutdown_drains_the_queue_and_later_misses_still_answer() {
    // Few enough writes to fit the drain budget on a loaded machine.
    const KEYS: usize = 8;
    let root = TempDir::new("shutdown");
    let keys: Vec<IconKey> = (0..KEYS)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    for (i, key) in keys.iter().enumerate() {
        cache
            .get_or_load(key, 48, || Ok(Some(png(i as u8, 2000))))
            .unwrap();
    }
    cache.shutdown();
    cache.shutdown();
    let stats = cache.stats();
    assert_eq!((stats.writes, stats.abandoned), (KEYS as u64, 0));
    assert_eq!(entries(&cache_dir(&root)).len(), KEYS);
    // After shutdown reads still work; nothing more is written.
    let extra = root.exe("extra.exe", b"MZ");
    assert_eq!(
        cache
            .get_or_load(&extra, 48, || Ok(Some(png(1, 10))))
            .unwrap(),
        Some(png(1, 10))
    );
    // A key this session loaded goes to the platform, as before.
    assert_eq!(
        cache
            .get_or_load(&keys[0], 48, || Ok(Some(png(0, 2000))))
            .unwrap(),
        Some(png(0, 2000))
    );
    cache.flush();
    assert_eq!(entries(&cache_dir(&root)).len(), KEYS);
}

#[test]
fn shutdown_waits_for_a_stuck_writer_only_so_long_and_its_queue_is_dropped() {
    let root = TempDir::new("stuck");
    let keys: Vec<IconKey> = (0..5)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    let cache = DiskIconCache::open(cache_dir(&root)).unwrap();
    cache.flush();
    // A file operation held up for 3 s (a file system filter, say),
    // with writes queued behind it.
    cache.tx.send(Job::Stall(Duration::from_secs(3))).unwrap();
    for (i, key) in keys.iter().enumerate() {
        cache
            .get_or_load(key, 48, || Ok(Some(png(i as u8, 200))))
            .unwrap();
    }
    let started = Instant::now();
    cache.shutdown();
    let took = started.elapsed();
    assert!(
        took >= SHUTDOWN_WAIT && took < Duration::from_millis(2000),
        "shutdown took {took:?}"
    );
    // Reads still answer while the writer is stuck.
    assert_eq!(
        cache
            .get_or_load(&keys[0], 48, || Ok(Some(png(0, 200))))
            .unwrap(),
        Some(png(0, 200))
    );

    // Once free, the writer drops what was queued instead of writing
    // it, and ends.
    let deadline = Instant::now() + Duration::from_secs(30);
    while cache.stats().abandoned < 5 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stats = cache.stats();
    assert_eq!((stats.abandoned, stats.writes), (5, 0));
    assert!(entries(&cache_dir(&root)).is_empty());
    let deadline = Instant::now() + Duration::from_secs(30);
    assert!(
        !matches!(
            cache.writer_stopped.recv_deadline(deadline),
            Err(RecvTimeoutError::Timeout)
        ),
        "the writer never ended"
    );
}
