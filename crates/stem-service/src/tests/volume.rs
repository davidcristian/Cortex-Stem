//! The volume item.

use pretty_assertions::assert_eq;

use super::*;

/// The demo with the volume item on, once the engine shows it.
fn start_with_volume() -> (DockService, Arc<MockPlatform>, Receiver<ServiceEvent>) {
    let (service, platform, rx) = start();
    let mut settings = service.settings();
    settings.items.volume = true;
    service.update_settings(settings).unwrap();
    wait_state(&rx, |s| s.items.iter().any(|i| i.key == "fixed:volume"));
    (service, platform, rx)
}

fn level(percent: u8, muted: bool) -> Option<VolumeStatus> {
    Some(VolumeStatus { percent, muted })
}

#[test]
fn a_scroll_over_the_volume_item_steps_it_two_points_a_notch_without_the_gate() {
    use ScrollDirection::{Next, Previous};
    let (service, platform, rx) = start_with_volume();
    assert_eq!(
        service.settings().behavior.scroll_action,
        stem_core::settings::ScrollAction::None
    );
    assert_eq!(service.volume(), level(60, false));
    let t0 = Instant::now();
    // Five notches up at the same instant: each counts, from the last.
    for _ in 0..5 {
        assert_eq!(
            service
                .scroll_item_at("fixed:volume", DOCK, Previous, t0)
                .unwrap(),
            None
        );
    }
    assert_eq!(platform.volume_status().unwrap(), level(70, false));
    let state = wait_state(&rx, |s| s.volume == level(70, false));
    assert_eq!(
        state
            .items
            .iter()
            .find(|i| i.key == "fixed:volume")
            .map(|i| i.title.as_str()),
        Some("Volume 70 %")
    );
    // Down, on another dock window too, and the gate of app items is
    // left alone.
    service
        .scroll_item_at("fixed:volume", "dock-DISPLAY3", Next, t0)
        .unwrap();
    assert_eq!(platform.volume_status().unwrap(), level(68, false));
    // One platform step per notch, never a read-then-set of its own.
    assert_eq!(
        platform.calls(),
        [Previous, Previous, Previous, Previous, Previous, Next]
            .into_iter()
            .map(MockCall::StepVolume)
            .collect::<Vec<_>>()
    );
    assert!(service.inner.scroll_gate.lock().last.is_empty());
    service.shutdown();
}

#[test]
fn a_volume_step_unmutes_going_up_clamps_and_does_nothing_without_a_device() {
    use ScrollDirection::{Next, Previous};
    let (service, platform, _rx) = start_with_volume();
    // Muted at 99: up raises to 100 and unmutes.
    platform.change_volume(level(99, true));
    assert_eq!(service.step_volume(Previous).unwrap(), level(100, false));
    assert_eq!(platform.volume_status().unwrap(), level(100, false));
    // At the top already: nothing changes.
    assert_eq!(service.step_volume(Previous).unwrap(), None);
    assert_eq!(platform.volume_status().unwrap(), level(100, false));
    // Muted at 1: down lowers to 0 and stays muted; then nothing more.
    platform.change_volume(level(1, true));
    assert_eq!(service.step_volume(Next).unwrap(), level(0, true));
    assert_eq!(service.step_volume(Next).unwrap(), None);
    assert_eq!(platform.volume_status().unwrap(), level(0, true));

    // No output device: the notch resolves and changes nothing.
    platform.change_volume(None);
    assert_eq!(
        service
            .scroll_item_at("fixed:volume", DOCK, Previous, Instant::now())
            .unwrap(),
        None
    );
    assert_eq!(platform.volume_status().unwrap(), None);
    assert!(
        platform
            .calls()
            .iter()
            .all(|c| matches!(c, MockCall::StepVolume(_)))
    );
    // The menu's Mute fails without a device; with one it is recorded.
    assert!(service.set_volume_muted(true).is_err());
    platform.change_volume(level(30, false));
    service.set_volume_muted(true).unwrap();
    assert_eq!(platform.volume_status().unwrap(), level(30, true));
    service.shutdown();
}

#[test]
fn a_volume_step_the_platform_does_not_answer_in_time_is_dropped_not_an_error() {
    // The relay's volume thread never answers in time.
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(Arc::clone(&relay));
    let mut settings = service.settings();
    settings.items.volume = true;
    service.update_settings(settings).unwrap();
    wait_state(&rx, |s| s.items.iter().any(|i| i.key == "fixed:volume"));
    for direction in [ScrollDirection::Previous, ScrollDirection::Next] {
        assert_eq!(
            service
                .scroll_item_at("fixed:volume", DOCK, direction, Instant::now())
                .unwrap(),
            None
        );
        assert_eq!(service.step_volume(direction).unwrap(), None);
    }
    assert!(service.inner.volume_stalled.load(Ordering::Relaxed));
    service.shutdown();
}

#[test]
fn a_scroll_over_the_volume_item_with_its_setting_off_is_an_unknown_item() {
    let (service, platform, rx) = start();
    wait_state(&rx, resolved);
    assert!(matches!(
        service.scroll_item_at("fixed:volume", DOCK, ScrollDirection::Next, Instant::now()),
        Err(ServiceError::UnknownItem(_))
    ));
    assert_eq!(platform.calls(), vec![]);
    service.shutdown();
}
