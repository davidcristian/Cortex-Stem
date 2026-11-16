use super::*;

#[test]
fn window_id_round_trip() {
    let id = WindowId(0x0001_0203);
    assert_eq!(window_id(hwnd(id)), id);
}
