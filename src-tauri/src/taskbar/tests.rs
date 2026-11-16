use std::path::PathBuf;

use stem_platform::rules::taskbar::Marker;

use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stem-taskbar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_marker_file_round_trips_in_the_documented_shape() {
    let dir = scratch_dir("round-trip");
    let store = MarkerStore::File(dir.join("nested").join(MARKER_FILE_NAME));
    assert_eq!(store.read(), None);
    store
        .write(Marker {
            previous_auto_hide: true,
        })
        .unwrap();
    let text = std::fs::read_to_string(dir.join("nested").join(MARKER_FILE_NAME)).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        serde_json::json!({ "previousAutoHide": true })
    );
    assert!(!dir.join("nested").join("taskbar.json.tmp").exists());
    assert_eq!(
        store.read(),
        Some(Marker {
            previous_auto_hide: true,
        })
    );
    store.delete().unwrap();
    assert_eq!(store.read(), None);
    store.delete().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_invalid_marker_file_counts_as_absent() {
    let dir = scratch_dir("invalid");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(MARKER_FILE_NAME);
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(MarkerStore::File(path).read(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_memory_store_holds_one_marker_and_touches_no_file() {
    let store = MarkerStore::Memory(Mutex::new(None));
    assert_eq!(store.read(), None);
    store
        .write(Marker {
            previous_auto_hide: false,
        })
        .unwrap();
    assert_eq!(
        store.read(),
        Some(Marker {
            previous_auto_hide: false,
        })
    );
    store.delete().unwrap();
    assert_eq!(store.read(), None);
    assert_eq!(store.to_string(), "<memory>");
}
