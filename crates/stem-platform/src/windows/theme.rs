//! OS theme facts.
//!
//! Contract for [`current_theme`]:
//! - `accent`: `DwmGetColorizationColor` or the registry value
//!   `HKCU\Software\Microsoft\Windows\DWM\AccentColor` (ABGR -> `#RRGGBB`).
//! - `dark`: `HKCU\...\Themes\Personalize\AppsUseLightTheme == 0`.
//! - `high_contrast`: `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`.
//! - `transparency`: `HKCU\...\Themes\Personalize\EnableTransparency`.
//!
//! Change notifications are delivered by the event thread (`events`), which
//! calls this function again on `WM_SETTINGCHANGE` / `WM_DWMCOLORIZATIONCOLORCHANGED`.

use stem_core::ThemeInfo;

use crate::{PlatformError, PlatformResult};

pub fn current_theme() -> PlatformResult<ThemeInfo> {
    Err(PlatformError::Unsupported("theme detection"))
}
