//! Performance: the part the mock platform can prove.
//!
//! The assertions are about behaviour (how many `State` notifications a
//! storm produces, what the final state is, which icon requests reach the
//! platform), never about how fast this host is. The timings are printed
//! for `docs/architecture/performance.md`:
//! `cargo test -p stem-service perf_ -- --nocapture --test-threads 1`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use pretty_assertions::assert_eq;
use stem_core::{
    AppId, AppIdentity, DockItemKind, DockState, IconKey, LaunchTarget, MonitorId, MonitorInfo,
    PlatformEvent, RawWindow, Rect, RecycleBinState, ShellAction, ThemeInfo, WindowAction,
    WindowId,
};
use stem_platform::mock::MockPlatform;
use stem_platform::{DockWindowOptions, EventSink, NativeWindowHandle, Platform, PlatformResult};

use crate::icon_cache::tests::{TempDir, png};
use crate::{
    DiskIconCache, DockService, MemorySettingsStore, STATE_EMIT_INTERVAL, ServiceEvent,
    ServiceOptions,
};

/// The mock, counting the calls the service makes that cost real work on
/// Windows.
#[derive(Debug)]
struct Counted {
    mock: MockPlatform,
    icon_calls: AtomicUsize,
    identify_calls: AtomicUsize,
}

impl Counted {
    fn over(mock: MockPlatform) -> Arc<Self> {
        Arc::new(Self {
            mock,
            icon_calls: AtomicUsize::new(0),
            identify_calls: AtomicUsize::new(0),
        })
    }

    fn icon_calls(&self) -> usize {
        self.icon_calls.load(Ordering::SeqCst)
    }

    fn identify_calls(&self) -> usize {
        self.identify_calls.load(Ordering::SeqCst)
    }
}

impl Platform for Counted {
    fn name(&self) -> &'static str {
        "counted-mock"
    }

    fn start(&self, sink: EventSink) -> PlatformResult<()> {
        self.mock.start(sink)
    }

    fn stop(&self) {
        self.mock.stop();
    }

    fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
        self.mock.windows()
    }

    fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
        self.mock.foreground_window()
    }

    fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
        self.mock.window_action(id, action)
    }

    fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
        self.identify_calls.fetch_add(1, Ordering::SeqCst);
        self.mock.identify_window(id)
    }

    fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity> {
        self.identify_calls.fetch_add(1, Ordering::SeqCst);
        self.mock.identify_target(target)
    }

    fn launch(&self, target: &LaunchTarget) -> PlatformResult<()> {
        self.mock.launch(target)
    }

    fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        self.icon_calls.fetch_add(1, Ordering::SeqCst);
        self.mock.icon_png(key, size)
    }

    fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
        self.mock.monitors()
    }

    fn theme(&self) -> PlatformResult<ThemeInfo> {
        self.mock.theme()
    }

    fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
        self.mock.recycle_bin()
    }

    fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()> {
        self.mock.delete_to_recycle_bin(paths)
    }

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        self.mock.shell_action(action)
    }

    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()> {
        self.mock.prepare_dock_window(handle, options)
    }
}

fn record(service: &DockService) -> Receiver<ServiceEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    service.subscribe(Box::new(move |ev| {
        let _ = tx.send(ev.clone());
    }));
    rx
}

/// The next `State` that satisfies `pred`, and how many `State`s arrived
/// up to and including it (each also handed to `each`).
fn states_until(
    rx: &Receiver<ServiceEvent>,
    mut each: impl FnMut(&DockState),
    pred: impl Fn(&DockState) -> bool,
) -> (DockState, usize) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = 0;
    loop {
        match rx.recv_deadline(deadline) {
            Ok(ServiceEvent::State(s)) => {
                seen += 1;
                each(&s);
                if pred(&s) {
                    return (s, seen);
                }
            }
            Ok(_) => {}
            Err(_) => panic!("state not reached after {seen} state notifications"),
        }
    }
}

fn demo_resolved(state: &DockState) -> bool {
    let apps: Vec<_> = state
        .items
        .iter()
        .filter(|i| i.kind == DockItemKind::App)
        .collect();
    apps.len() == 5 && apps.iter().all(|i| i.icon_key.is_some())
}

const NOTEPAD: &str = r"C:\Windows\System32\notepad.exe";
const STORM: u64 = 200;
const FIRST_STORM_ID: u64 = 10_000;

fn notepad_window(i: u64) -> RawWindow {
    RawWindow {
        id: WindowId(FIRST_STORM_ID + i),
        title: format!("Untitled {i} - Notepad"),
        class_name: "Notepad".into(),
        process_id: 20_000 + u32::try_from(i).unwrap(),
        exe_path: Some(PathBuf::from(NOTEPAD)),
        app_user_model_id: None,
        is_visible: true,
        is_minimized: false,
        is_cloaked: false,
        on_current_desktop: Some(true),
        monitor: Some(MonitorId("mock-primary".into())),
        rect: Some(Rect::new(100, 100, 800, 600)),
        icon_key: None,
    }
}

/// How many Notepad windows `state` lists.
fn notepads(state: &DockState) -> usize {
    let notepad = AppId::for_exe(Path::new(NOTEPAD));
    state
        .items
        .iter()
        .filter(|i| i.app_id.as_ref() == Some(&notepad))
        .map(|i| i.windows.len())
        .sum()
}

/// The rate limit's bound on `State` notifications over `elapsed`: one per
/// interval, plus the first and the one already due when the phase began.
fn emit_bound(elapsed: Duration) -> usize {
    usize::try_from(elapsed.as_millis() / STATE_EMIT_INTERVAL.as_millis()).unwrap() + 2
}

#[test]
fn perf_a_storm_of_200_windows_opening_and_closing_is_coalesced_and_ends_where_it_began() {
    let platform = Counted::over(MockPlatform::with_demo_scenario());
    let service =
        DockService::start(platform.clone(), Box::new(MemorySettingsStore::default())).unwrap();
    let rx = record(&service);
    states_until(&rx, |_| {}, demo_resolved);
    // Let the start settle, then forget what it emitted.
    std::thread::sleep(Duration::from_millis(100));
    while rx.try_recv().is_ok() {}
    let before = service.state();
    let identify_before = platform.identify_calls();

    // Open: each window arrives and takes the foreground, as a new Notepad
    // window does, as fast as the mock delivers the events.
    let started = Instant::now();
    for i in 0..STORM {
        let window = notepad_window(i);
        let id = window.id;
        platform.mock.push(PlatformEvent::WindowAdded(window));
        platform
            .mock
            .push(PlatformEvent::ForegroundChanged(Some(id)));
    }
    let pushed_open = started.elapsed();
    let mut last = 0;
    let (opened, open_states) = states_until(
        &rx,
        |s| {
            let n = notepads(s);
            assert!(n >= last, "the Notepad count went back from {last} to {n}");
            last = n;
        },
        |s| notepads(s) == 200 && s.foreground == Some(WindowId(FIRST_STORM_ID + STORM - 1)),
    );
    let open_took = started.elapsed();
    let notepad_item = opened
        .items
        .iter()
        .find(|i| i.windows.len() == 200)
        .unwrap();
    assert!(notepad_item.is_active);
    assert_eq!(
        notepad_item.windows.first().unwrap().id,
        WindowId(FIRST_STORM_ID)
    );

    // Close: each closes and the next one takes the foreground; the last
    // hands it back to the window that had it before the storm.
    let started = Instant::now();
    for i in 0..STORM {
        platform
            .mock
            .push(PlatformEvent::WindowRemoved(WindowId(FIRST_STORM_ID + i)));
        let next = if i + 1 < STORM {
            WindowId(FIRST_STORM_ID + i + 1)
        } else {
            before.foreground.unwrap()
        };
        platform
            .mock
            .push(PlatformEvent::ForegroundChanged(Some(next)));
    }
    let pushed_close = started.elapsed();
    let mut last = 200;
    let (closed, close_states) = states_until(
        &rx,
        |s| {
            let n = notepads(s);
            assert!(n <= last, "the Notepad count went up from {last} to {n}");
            last = n;
        },
        |s| notepads(s) == 0 && s.foreground == before.foreground,
    );
    let close_took = started.elapsed();

    // Coalesced: never more than the rate limit lets out, and far fewer
    // notifications than the 400 events of each phase.
    assert!(
        open_states <= emit_bound(open_took),
        "{open_states} states while opening in {open_took:?}"
    );
    assert!(
        close_states <= emit_bound(close_took),
        "{close_states} states while closing in {close_took:?}"
    );
    assert!(open_states <= 100 && close_states <= 100);
    // One identity lookup for Notepad (plus at most its retry), not one per
    // window.
    let lookups = platform.identify_calls() - identify_before;
    assert!(lookups <= 2, "{lookups} identity lookups for one app");

    // The dock is exactly where it was, and the engine agrees with the last
    // notification.
    assert_eq!(closed.items, before.items);
    assert_eq!(closed.foreground, before.foreground);
    std::thread::sleep(Duration::from_millis(100));
    let after = service.state();
    assert_eq!(after.items, before.items);
    assert_eq!(after.foreground, before.foreground);
    service.shutdown();

    eprintln!(
        "storm: {STORM} windows opened ({} events pushed in {pushed_open:?}; all 200 shown after {open_took:?} in {open_states} state notifications), \
         then closed ({} events pushed in {pushed_close:?}; back to the start after {close_took:?} in {close_states} state notifications); \
         {lookups} identity lookup(s)",
        STORM * 2,
        STORM * 2
    );
}

/// Each call's duration, sorted.
fn time_each(keys: &[IconKey], mut call: impl FnMut(&IconKey)) -> Vec<Duration> {
    let mut times: Vec<Duration> = keys
        .iter()
        .map(|key| {
            let started = Instant::now();
            call(key);
            started.elapsed()
        })
        .collect();
    times.sort();
    times
}

fn summary(times: &[Duration]) -> String {
    let at = |q: f64| {
        let index = ((times.len() - 1) as f64 * q).round() as usize;
        times[index].as_secs_f64() * 1e6
    };
    format!(
        "median {:.0} µs, p95 {:.0} µs, max {:.0} µs",
        at(0.5),
        at(0.95),
        at(1.0)
    )
}

fn cache_stats(service: &DockService) -> crate::icon_cache::CacheStats {
    service.inner.icon_cache.as_ref().unwrap().stats()
}

fn with_cache(platform: Arc<Counted>, dir: &Path) -> DockService {
    DockService::start_with(
        platform,
        Box::new(MemorySettingsStore::default()),
        ServiceOptions {
            icon_cache_dir: Some(dir.to_path_buf()),
        },
    )
    .unwrap()
}

#[test]
fn perf_icon_requests_hit_the_disk_cache_after_a_restart() {
    const APPS: usize = 100;
    let root = TempDir::new("perf-service");
    let dir = root.path().join("icons");
    let keys: Vec<IconKey> = (0..APPS)
        .map(|i| root.exe(&format!("app{i}.exe"), format!("MZ app {i}").as_bytes()))
        .collect();

    for size in [96, 256] {
        // Today's behaviour, for comparison: no cache directory.
        let platform = Counted::over(MockPlatform::new());
        let service =
            DockService::start(platform.clone(), Box::new(MemorySettingsStore::default())).unwrap();
        let expected: Vec<Vec<u8>> = keys
            .iter()
            .map(|k| service.icon_png(k, size).unwrap().unwrap())
            .collect();
        let none = time_each(&keys, |k| {
            service.icon_png(k, size).unwrap();
        });
        service.shutdown();
        assert_eq!(platform.icon_calls(), 2 * APPS);

        // Cold: an empty cache directory. Every request is a miss that also
        // queues a write.
        let _ = std::fs::remove_dir_all(&dir);
        let platform = Counted::over(MockPlatform::new());
        let service = with_cache(platform.clone(), &dir);
        let cold = time_each(&keys, |k| {
            service.icon_png(k, size).unwrap();
        });
        // `shutdown` drains the writer's queue.
        service.shutdown();
        let written = cache_stats(&service);
        assert_eq!(platform.icon_calls(), APPS);
        assert_eq!((written.misses, written.writes), (APPS as u64, APPS as u64));
        assert_eq!(written.entries, APPS as u64);

        // Warm: a restart over the same directory. No request reaches the
        // platform, and every answer is the platform's bitmap. The second
        // round is the session's repeats, answered from memory.
        let platform = Counted::over(MockPlatform::new());
        let service = with_cache(platform.clone(), &dir);
        let mut answers = Vec::with_capacity(APPS);
        let warm = time_each(&keys, |k| {
            answers.push(service.icon_png(k, size).unwrap().unwrap());
        });
        assert_eq!(answers, expected);
        let repeat = time_each(&keys, |k| {
            service.icon_png(k, size).unwrap();
        });
        service.shutdown();
        let read = cache_stats(&service);
        assert_eq!(platform.icon_calls(), 0);
        assert_eq!(
            (read.hits, read.repeats, read.misses),
            (APPS as u64, APPS as u64, 0)
        );
        let stamps = time_each(&keys, |k| {
            crate::icon_cache::Stamp::of(crate::icon_cache::source_file(k).unwrap()).unwrap();
        });

        eprintln!(
            "icons at {size} px ({} bytes each), {APPS} apps: no cache {}; cold miss + queued write {}; \
             writer {:.0} µs per entry (off the caller's thread); warm hit {} (of which the source file's metadata {}); \
             repeat in the session {}",
            expected[0].len(),
            summary(&none),
            summary(&cold),
            written.write_nanos as f64 / written.writes as f64 / 1e3,
            summary(&warm),
            summary(&stamps),
            summary(&repeat)
        );
    }
}

#[test]
fn perf_a_hit_on_a_large_icon_reads_and_checks_the_whole_entry() {
    // Real 256 px icons are 20–80 KB of PNG (the mock's solid squares are
    // a few hundred bytes); 64 KiB is the cost of a large one.
    const APPS: usize = 100;
    let root = TempDir::new("perf-large");
    let dir = root.path().join("icons");
    let keys: Vec<IconKey> = (0..APPS)
        .map(|i| root.exe(&format!("app{i}.exe"), b"MZ"))
        .collect();
    let payload = png(0x5A, 64 * 1024);
    {
        let cache = DiskIconCache::open(&dir).unwrap();
        let cold = time_each(&keys, |k| {
            cache
                .get_or_load(k, 256, || Ok(Some(payload.clone())))
                .unwrap();
        });
        cache.flush();
        let stats = cache.stats();
        assert_eq!(stats.writes, APPS as u64);
        eprintln!(
            "64 KiB icons, {APPS} apps: miss + queued write {}; writer {:.0} µs per entry",
            summary(&cold),
            stats.write_nanos as f64 / stats.writes as f64 / 1e3
        );
    }
    let cache = DiskIconCache::open(&dir).unwrap();
    let warm = time_each(&keys, |k| {
        let got = cache.get_or_load(k, 256, || Ok(None)).unwrap();
        assert_eq!(got.map(|p| p.len()), Some(payload.len()));
    });
    assert_eq!(cache.stats().hits, APPS as u64);
    eprintln!("64 KiB icons, {APPS} apps: warm hit {}", summary(&warm));
}

/// On a Windows host: the real platform's extraction (PE resources,
/// downscale, PNG encode; file reads and GDI only, no window is touched)
/// against a disk hit, for a few system executables. Each run uses a fresh
/// `WindowsPlatform`, so its memory cache is empty, as at a start.
#[cfg(windows)]
#[test]
fn perf_real_extraction_against_a_disk_hit_on_windows() {
    use stem_platform::windows::WindowsPlatform;

    let root = std::env::var("SystemRoot").unwrap();
    let keys: Vec<IconKey> = [
        "explorer.exe",
        "regedit.exe",
        "System32\\cmd.exe",
        "System32\\notepad.exe",
        "System32\\taskmgr.exe",
        "System32\\mmc.exe",
        "System32\\charmap.exe",
        "System32\\control.exe",
        "System32\\msinfo32.exe",
        "System32\\perfmon.exe",
        "System32\\resmon.exe",
        "System32\\mstsc.exe",
    ]
    .iter()
    .map(|file| format!("{root}\\{file}"))
    .filter(|path| Path::new(path).is_file())
    .map(|path| IconKey(format!("exe:{path}")))
    .collect();
    assert!(keys.len() >= 4, "{keys:?}");
    let temp = TempDir::new("perf-windows");
    let dir = temp.path().join("icons");

    for size in [96, 256] {
        let platform = WindowsPlatform::new();
        let none = time_each(&keys, |k| {
            assert!(platform.icon_png(k, size).unwrap().is_some(), "{k:?}");
        });

        let _ = std::fs::remove_dir_all(&dir);
        let platform = WindowsPlatform::new();
        let cache = DiskIconCache::open(&dir).unwrap();
        let cold = time_each(&keys, |k| {
            cache
                .get_or_load(k, size, || platform.icon_png(k, size))
                .unwrap();
        });
        cache.shutdown();
        let written = cache.stats();
        assert_eq!(written.writes, keys.len() as u64);

        let platform = WindowsPlatform::new();
        let cache = DiskIconCache::open(&dir).unwrap();
        let warm = time_each(&keys, |k| {
            cache
                .get_or_load(k, size, || platform.icon_png(k, size))
                .unwrap();
        });
        assert_eq!(cache.stats().hits, keys.len() as u64);
        let stamps = time_each(&keys, |k| {
            crate::icon_cache::Stamp::of(crate::icon_cache::source_file(k).unwrap()).unwrap();
        });
        eprintln!(
            "windows, {} system exes at {size} px ({} bytes on disk): extraction {}; miss + queued write {}; \
             writer {:.0} µs per entry; disk hit {} (of which the source file's metadata {})",
            keys.len(),
            written.bytes,
            summary(&none),
            summary(&cold),
            written.write_nanos as f64 / written.writes as f64 / 1e3,
            summary(&warm),
            summary(&stamps)
        );
    }
}

#[test]
fn without_a_cache_directory_every_icon_request_reaches_the_platform() {
    let root = TempDir::new("no-dir");
    let key = root.exe("app.exe", b"MZ");
    let platform = Counted::over(MockPlatform::new());
    let service = DockService::start_with(
        platform.clone(),
        Box::new(MemorySettingsStore::default()),
        ServiceOptions::default(),
    )
    .unwrap();
    let first = service.icon_png(&key, 48).unwrap();
    assert_eq!(service.icon_png(&key, 48).unwrap(), first);
    assert_eq!(platform.icon_calls(), 2);
    assert!(service.inner.icon_cache.is_none());
    service.shutdown();
}

#[test]
fn a_cache_directory_that_cannot_be_created_leaves_icons_to_the_platform() {
    let root = TempDir::new("bad-dir");
    let blocker = root.path().join("not-a-dir");
    std::fs::write(&blocker, b"a file").unwrap();
    let key = root.exe("app.exe", b"MZ");
    let platform = Counted::over(MockPlatform::with_demo_scenario());
    let service = with_cache(platform.clone(), &blocker.join("icons"));
    assert!(service.inner.icon_cache.is_none());
    assert!(service.icon_png(&key, 48).unwrap().is_some());
    assert!(service.icon_png(&key, 48).unwrap().is_some());
    assert_eq!(platform.icon_calls(), 2);
    // The rest of the service is unaffected.
    let rx = record(&service);
    platform
        .mock
        .push(PlatformEvent::WindowAdded(notepad_window(1)));
    states_until(&rx, |_| {}, |s| notepads(s) == 1);
    service.shutdown();
}

#[test]
fn window_and_shell_icons_go_to_the_platform_even_with_a_cache() {
    let root = TempDir::new("service-uncached");
    let platform = Counted::over(MockPlatform::new());
    let service = with_cache(platform.clone(), &root.path().join("icons"));
    for key in [
        IconKey("hwnd:42:7".into()),
        IconKey::shell("recycle-bin-full"),
        IconKey("aumid:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
        IconKey("mock:app:x".into()),
    ] {
        let first = service.icon_png(&key, 48).unwrap();
        assert_eq!(service.icon_png(&key, 48).unwrap(), first);
    }
    assert_eq!(platform.icon_calls(), 8);
    service.shutdown();
    assert_eq!(cache_stats(&service).writes, 0);
}
