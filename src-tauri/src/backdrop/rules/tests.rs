use super::*;

#[test]
fn the_setting_picks_the_material_and_a_contrast_theme_refuses_it() {
    assert_eq!(effective(Backdrop::Css, false), Effective::Css);
    assert_eq!(effective(Backdrop::Css, true), Effective::Css);
    assert_eq!(
        effective(Backdrop::Acrylic, false),
        Effective::Native(BackdropMaterial::Acrylic)
    );
    assert_eq!(
        effective(Backdrop::Mica, false),
        Effective::Native(BackdropMaterial::Mica)
    );
    assert_eq!(
        effective(Backdrop::Acrylic, true),
        Effective::ContrastTheme(BackdropMaterial::Acrylic)
    );
}

#[test]
fn the_material_is_dark_as_the_theme_setting_says() {
    let light = ThemeInfo {
        dark: false,
        ..ThemeInfo::default()
    };
    let night = ThemeInfo {
        dark: true,
        ..ThemeInfo::default()
    };
    assert!(!dark(ThemeMode::System, &light));
    assert!(dark(ThemeMode::System, &night));
    assert!(dark(ThemeMode::Dark, &light));
    assert!(!dark(ThemeMode::Light, &night));
}

#[test]
fn the_panel_is_rounded_inwards_and_offset_by_the_window() {
    assert_eq!(
        panel_inwards((0, 1248), Rect::new(360, 168, 561, 64), 1.0),
        Rect::new(360, 1416, 561, 64)
    );
    // 11 * 1.5 = 16.5 -> 17; (11 + 10) * 1.5 = 31.5 -> 31.
    assert_eq!(
        panel_inwards((100, 2000), Rect::new(11, 0, 10, 8), 1.5),
        Rect::new(117, 2000, 14, 12)
    );
    assert_eq!(
        panel_inwards((-1920, 944), Rect::new(600, 20, 500, 70), 1.25),
        Rect::new(-1170, 969, 625, 87)
    );
    // An unusable scale counts as 1; a sliver collapses to nothing.
    assert_eq!(
        panel_inwards((0, 0), Rect::new(5, 5, 10, 10), f64::NAN),
        Rect::new(5, 5, 10, 10)
    );
    assert_eq!(
        panel_inwards((0, 0), Rect::new(1, 1, 0, 0), 1.5).width,
        0,
        "never negative"
    );
}

fn place() -> Option<PanelPlace> {
    Some(PanelPlace {
        rect: Rect::new(800, 1300, 960, 96),
        scale: 1.5,
    })
}

#[test]
fn a_native_material_on_a_shown_panel_is_requested_at_the_panels_scale() {
    assert_eq!(
        wanted(
            Effective::Native(BackdropMaterial::Acrylic),
            place(),
            false,
            22,
            true
        ),
        Some(DockBackdrop {
            rect: Rect::new(800, 1300, 960, 96),
            corner_radius: 33,
            material: BackdropMaterial::Acrylic,
            dark: true,
        })
    );
}

#[test]
fn nothing_is_requested_without_a_material_a_panel_or_while_slid_out() {
    let acrylic = Effective::Native(BackdropMaterial::Acrylic);
    assert_eq!(wanted(Effective::Css, place(), false, 22, false), None);
    assert_eq!(
        wanted(
            Effective::ContrastTheme(BackdropMaterial::Mica),
            place(),
            false,
            22,
            false
        ),
        None
    );
    assert_eq!(wanted(acrylic, None, false, 22, false), None, "hidden");
    assert_eq!(wanted(acrylic, place(), true, 22, false), None, "slid out");
    let empty = Some(PanelPlace {
        rect: Rect::new(800, 1300, 0, 96),
        scale: 1.0,
    });
    assert_eq!(wanted(acrylic, empty, false, 22, false), None);
}
