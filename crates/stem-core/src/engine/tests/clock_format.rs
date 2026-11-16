//! The clock's regional format.

use pretty_assertions::assert_eq;

use super::*;

fn clock_format(locale: Option<&str>, hour12: Option<bool>) -> ClockFormat {
    ClockFormat {
        locale: locale.map(str::to_owned),
        hour12,
    }
}

fn format_changed(format: ClockFormat) -> EngineInput {
    PlatformEvent::ClockFormatChanged(format).into()
}

#[test]
fn clock_format_is_unknown_until_the_first_reading_and_then_kept_as_sent() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, false));
    assert_eq!(engine.clock_format(), &ClockFormat::default());
    let state = engine.snapshot();
    assert_eq!(state.clock_format.locale, None);
    assert_eq!(state.clock_format.hour12, None);

    // The first reading equal to "nothing known" changes nothing.
    let r = state.revision;
    assert!(!engine.apply(format_changed(ClockFormat::default())).changed);
    assert_eq!(engine.snapshot().revision, r);

    let british = clock_format(Some("en-GB"), Some(false));
    assert!(engine.apply(format_changed(british.clone())).changed);
    assert_eq!(engine.clock_format(), &british);
    assert_eq!(engine.snapshot().clock_format, british);
    // Stored as is: the engine does not judge the name.
    let odd = clock_format(Some("not a locale"), None);
    assert!(engine.apply(format_changed(odd.clone())).changed);
    assert_eq!(engine.snapshot().clock_format, odd);
}

#[test]
fn clock_format_changed_bumps_the_revision_only_when_the_reading_differs() {
    let mut engine = DockEngine::new(with_clock_and_battery(true, false));
    let american = clock_format(Some("en-US"), Some(true));
    assert!(engine.apply(format_changed(american.clone())).changed);
    let r = engine.snapshot().revision;

    // The same reading again (another `intl` broadcast): nothing.
    assert!(!engine.apply(format_changed(american)).changed);
    assert_eq!(engine.snapshot().revision, r);

    // Each field on its own is a change: a custom 24-hour short time in
    // the same region, then another region, then a reading that says
    // less.
    assert!(
        engine
            .apply(format_changed(clock_format(Some("en-US"), Some(false))))
            .changed
    );
    assert!(
        engine
            .apply(format_changed(clock_format(Some("de-DE"), Some(false))))
            .changed
    );
    assert!(
        engine
            .apply(format_changed(clock_format(Some("de-DE"), None)))
            .changed
    );
    assert!(
        engine
            .apply(format_changed(clock_format(None, None)))
            .changed
    );
    assert_eq!(engine.snapshot().revision, r + 4);

    // `DockState::clock_format` is the platform's fact whatever the
    // settings say: without the clock the snapshot keeps it, but no item
    // shows it, so it bumps nothing, and no item moves.
    let mut engine = DockEngine::new(Settings::default());
    let before = engine.snapshot();
    assert!(
        !engine
            .apply(format_changed(clock_format(Some("fr-FR"), Some(false))))
            .changed
    );
    assert_eq!(
        engine.snapshot().clock_format.locale.as_deref(),
        Some("fr-FR")
    );
    assert_eq!(engine.snapshot().revision, before.revision);
    assert_eq!(engine.snapshot().items, before.items);
}

#[test]
fn a_regional_format_is_kept_without_a_new_revision_while_no_clock_is_shown() {
    // The clock off, readings come in (the region changed twice).
    let mut settings = with_clock_and_battery(false, false);
    let mut engine = DockEngine::new(settings.clone());
    let r = engine.snapshot().revision;
    let german = clock_format(Some("de-DE"), Some(false));
    assert!(
        !engine
            .apply(format_changed(clock_format(Some("en-US"), Some(true))))
            .changed
    );
    assert!(!engine.apply(format_changed(german.clone())).changed);
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.clock_format(), &german);

    // The next state sent for another reason carries the latest one.
    assert!(engine.apply(added(win(1, r"C:\a.exe", "A"))).changed);
    let state = engine.snapshot();
    assert_eq!(state.revision, r + 1);
    assert_eq!(state.clock_format, german);

    // The clock switched on shows the reading kept at once: the state
    // that brings the item carries it.
    settings.items.clock = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    let state = engine.snapshot();
    assert!(item(&state, "fixed:clock").is_some());
    assert_eq!(state.clock_format, german);
    // While it is shown, every new reading is a change.
    let r = state.revision;
    assert!(
        engine
            .apply(format_changed(clock_format(Some("de-DE"), Some(true))))
            .changed
    );
    assert_eq!(engine.snapshot().revision, r + 1);

    // It counts on the clock only: the battery item shown does not make
    // a format change count, and the clock shown does not make a power
    // reading count while the battery item is off.
    settings.items.clock = false;
    settings.items.battery = true;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    engine.apply(power(laptop(50)));
    assert!(item(&engine.snapshot(), "fixed:battery").is_some());
    assert!(
        !engine
            .apply(format_changed(clock_format(Some("fr-FR"), None)))
            .changed
    );
    settings.items.clock = true;
    settings.items.battery = false;
    assert!(engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert_eq!(
        engine.snapshot().clock_format,
        clock_format(Some("fr-FR"), None)
    );
    assert!(!engine.apply(power(laptop(49))).changed);
    assert!(
        engine
            .apply(format_changed(clock_format(Some("fr-FR"), Some(false))))
            .changed
    );
}

#[test]
fn the_clock_item_is_the_same_in_any_regional_format() {
    let mut settings = with_clock_and_battery(true, true);
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(power(laptop(60)));
    let items = engine.snapshot().items;
    let clock = item(&engine.snapshot(), "fixed:clock").cloned().unwrap();
    assert_eq!(clock.title, "Clock");

    // The page formats the time; Rust's items do not change with it.
    engine.apply(format_changed(clock_format(Some("ko-KR"), Some(true))));
    assert_eq!(engine.snapshot().items, items);
    assert_eq!(
        engine.primary_action("fixed:clock", DOCK),
        ItemAction::Shell(ShellAction::OpenNotificationCenter)
    );

    // The reading is kept across setting changes, the clock's own too.
    settings.items.clock = false;
    assert!(
        engine
            .apply(EngineInput::SettingsChanged(settings.clone()))
            .changed
    );
    settings.items.clock = true;
    engine.apply(EngineInput::SettingsChanged(settings));
    assert_eq!(
        engine.clock_format(),
        &clock_format(Some("ko-KR"), Some(true))
    );
    assert_eq!(item(&engine.snapshot(), "fixed:clock"), Some(&clock));
}
