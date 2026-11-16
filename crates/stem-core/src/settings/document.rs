//! The settings document as text: parsing it with migration and
//! validation, and writing it back.

use super::{Appearance, Magnification, SETTINGS_VERSION, Settings, SettingsError};
use crate::model::AppId;

impl Settings {
    /// Parse, migrate, and validate a settings document. A leading byte
    /// order mark (U+FEFF, which some editors write) is ignored, as RFC 8259
    /// lets a parser do.
    pub fn from_json(json: &str) -> Result<Self, SettingsError> {
        let json = json.strip_prefix('\u{feff}').unwrap_or(json);
        let mut settings: Self = serde_json::from_str(json)?;
        if settings.version > SETTINGS_VERSION {
            return Err(SettingsError::TooNew {
                found: settings.version,
                supported: SETTINGS_VERSION,
            });
        }
        settings.migrate();
        settings.validate();
        Ok(settings)
    }

    pub fn to_json_pretty(&self) -> String {
        // Serialising our own plain data cannot fail: `validate` guarantees
        // every float is finite, which is the only thing JSON cannot encode.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Upgrade an older document in place. Each new version adds a step.
    pub fn migrate(&mut self) {
        // v0 (no version field) -> v1: nothing to do beyond defaults.
        self.version = SETTINGS_VERSION;
    }

    /// Clamp every numeric field into its supported range, replace
    /// non-finite floats with their defaults, drop invalid pins (blank or
    /// duplicate ids; the first occurrence stays), and drop hidden apps that
    /// are pinned, have a blank id or repeat an id (the first
    /// occurrence stays). Idempotent: validating a validated document changes
    /// nothing.
    pub fn validate(&mut self) {
        self.icon_size = self.icon_size.clamp(24, 128);
        self.magnification.max_scale = finite_or(
            self.magnification.max_scale,
            Magnification::default().max_scale,
        )
        .clamp(1.0, 2.5);
        self.magnification.range_icons = self.magnification.range_icons.clamp(1, 6);
        self.auto_hide.delay_ms = self.auto_hide.delay_ms.min(5_000);
        self.appearance.opacity =
            finite_or(self.appearance.opacity, Appearance::default().opacity).clamp(0.0, 1.0);
        self.appearance.corner_radius = self.appearance.corner_radius.min(64);
        let mut seen = std::collections::HashSet::new();
        self.pinned
            .retain(|p| !p.id.as_str().trim().is_empty() && seen.insert(p.id.clone()));
        // After the pins, which take precedence: a pinned app is never hidden.
        let pinned: std::collections::HashSet<AppId> =
            self.pinned.iter().map(|p| p.id.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        self.hidden_apps.retain(|h| {
            !h.id.as_str().trim().is_empty() && !pinned.contains(&h.id) && seen.insert(h.id.clone())
        });
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}
