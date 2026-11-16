//! UTF-16 buffers, the form Win32 takes and returns strings in.
//!
//! `windows::util` re-exports these for the Windows code, which passes
//! [`to_wide`]'s buffer as a `PCWSTR` and decodes what a call wrote with
//! [`from_wide`]. Plain `u16` slices, so they are unit-tested on every host.

/// Build a NUL-terminated UTF-16 buffer for a `PCWSTR` argument.
/// Keep the returned `Vec` alive for as long as the pointer is used.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a UTF-16 buffer up to the first NUL (or its end).
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_round_trip() {
        let w = to_wide("Stem ✓");
        assert_eq!(w.last(), Some(&0));
        assert_eq!(from_wide(&w), "Stem ✓");
        assert_eq!(from_wide(&[0x44, 0x6f, 0x00, 0x63]), "Do");
    }
}
