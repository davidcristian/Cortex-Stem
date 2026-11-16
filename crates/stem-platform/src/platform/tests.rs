use stem_core::{
    AppIdentity, IconKey, LaunchTarget, MonitorInfo, RawWindow, RecycleBinState, ScrollDirection,
    ShellAction, ThemeInfo, WindowAction, WindowId,
};

use super::*;

#[test]
fn is_mock_recognises_the_mock_platform_by_name() {
    let mock: Arc<dyn Platform> = Arc::new(mock::MockPlatform::new());
    assert!(is_mock(mock.as_ref()));
    assert_eq!(mock.name(), MOCK_PLATFORM_NAME);

    struct Real;
    impl Platform for Real {
        fn name(&self) -> &'static str {
            "windows"
        }
        fn start(&self, _sink: EventSink) -> PlatformResult<()> {
            Ok(())
        }
        fn stop(&self) {}
        fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
            Ok(vec![])
        }
        fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
            Ok(None)
        }
        fn window_action(&self, _id: WindowId, _action: WindowAction) -> PlatformResult<()> {
            Ok(())
        }
        fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
            Err(PlatformError::WindowNotFound(id))
        }
        fn identify_target(&self, _target: &LaunchTarget) -> PlatformResult<AppIdentity> {
            Err(PlatformError::Unsupported("test"))
        }
        fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
            Ok(())
        }
        fn icon_png(&self, _key: &IconKey, _size: u32) -> PlatformResult<Option<Vec<u8>>> {
            Ok(None)
        }
        fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
            Ok(vec![])
        }
        fn theme(&self) -> PlatformResult<ThemeInfo> {
            Ok(ThemeInfo::default())
        }
        fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
            Ok(RecycleBinState::default())
        }
        fn delete_to_recycle_bin(&self, _paths: &[String]) -> PlatformResult<()> {
            Ok(())
        }
        fn shell_action(&self, _action: ShellAction) -> PlatformResult<()> {
            Ok(())
        }
        fn prepare_dock_window(
            &self,
            _handle: NativeWindowHandle,
            _options: DockWindowOptions,
        ) -> PlatformResult<()> {
            Ok(())
        }
    }
    assert!(!is_mock(&Real));
    // A platform that does not implement the focus shortcut says so,
    // naming the feature, instead of pretending it worked.
    assert!(matches!(
        Real.focus_dock_window(NativeWindowHandle(0x1234)),
        Err(PlatformError::Unsupported("keyboard focus"))
    ));
    // The same for the power reading.
    assert!(matches!(
        Real.power_status(),
        Err(PlatformError::Unsupported("widget support"))
    ));
    // And for the regional format.
    assert!(matches!(
        Real.clock_format(),
        Err(PlatformError::Unsupported("widget support"))
    ));
    // And for the volume and its two actions.
    assert!(matches!(
        Real.volume_status(),
        Err(PlatformError::Unsupported("widget support"))
    ));
    assert!(matches!(
        Real.set_volume(50),
        Err(PlatformError::Unsupported("widget support"))
    ));
    assert!(matches!(
        Real.set_volume_muted(true),
        Err(PlatformError::Unsupported("widget support"))
    ));
    assert!(matches!(
        Real.step_volume(ScrollDirection::Previous),
        Err(PlatformError::Unsupported("widget support"))
    ));
}
