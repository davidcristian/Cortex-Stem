//! Entry file names and the entry format: encoding, the checksum and
//! decoding.

use stem_core::IconKey;

use super::{
    ENTRY_EXTENSION, FNV_OFFSET, FNV_PRIME, FORMAT_VERSION, MAGIC, NAME_DIGITS, PNG_SIGNATURE,
    Stamp, TEMP_EXTENSION,
};

/// `<FNV-1a 64 of the key, a NUL and the size>.icon`.
pub(super) fn entry_name(key: &IconKey, size: u32) -> String {
    let hash = key
        .0
        .bytes()
        .chain([0])
        .chain(size.to_le_bytes())
        .fold(FNV_OFFSET, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(FNV_PRIME)
        });
    format!("{hash:016x}{ENTRY_EXTENSION}")
}

pub(super) fn temp_name(entry: &str, seq: u64) -> String {
    let stem = entry.strip_suffix(ENTRY_EXTENSION).unwrap_or(entry);
    format!("{stem}.{}-{seq}{TEMP_EXTENSION}", std::process::id())
}

fn is_hex_stem(name: &str) -> bool {
    name.len() >= NAME_DIGITS
        && name.as_bytes()[..NAME_DIGITS]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn is_entry_name(name: &str) -> bool {
    name.len() == NAME_DIGITS + ENTRY_EXTENSION.len()
        && is_hex_stem(name)
        && name.ends_with(ENTRY_EXTENSION)
}

pub(super) fn is_temp_name(name: &str) -> bool {
    is_hex_stem(name)
        && name.as_bytes().get(NAME_DIGITS) == Some(&b'.')
        && name.ends_with(TEMP_EXTENSION)
}

/// A 64-bit checksum against torn writes and bit rot (not an adversary):
/// FNV-style mixing over 8-byte words, so a hit costs little even for a
/// large PNG. Every step is a bijection of the state, so a change confined
/// to one word always changes the result.
fn checksum(bytes: &[u8]) -> u64 {
    let words = bytes.chunks_exact(8);
    let rest = words.remainder();
    let mut h = FNV_OFFSET ^ u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    for word in words {
        // Every chunk of `chunks_exact(8)` has 8 bytes.
        let word: [u8; 8] = word.try_into().unwrap_or_default();
        h = (h ^ u64::from_le_bytes(word))
            .wrapping_mul(FNV_PRIME)
            .rotate_left(29);
    }
    for &b in rest {
        h = (h ^ u64::from(b)).wrapping_mul(FNV_PRIME);
    }
    h
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    // Callers bound every field far below 4 GiB; a length that did not fit
    // would make the entry fail to decode, which is a miss.
    let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
}

pub(super) fn encode(producer: &str, key: &str, size: u32, stamp: Stamp, png: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + producer.len() + key.len() + png.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    put_bytes(&mut out, producer.as_bytes());
    put_bytes(&mut out, key.as_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&stamp.len.to_le_bytes());
    out.extend_from_slice(&stamp.modified_ns.to_le_bytes());
    put_bytes(&mut out, png);
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Decoded<'a> {
    pub(super) producer: &'a [u8],
    pub(super) key: &'a [u8],
    pub(super) size: u32,
    pub(super) stamp: Stamp,
    pub(super) png: &'a [u8],
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.0.split_first_chunk::<N>()?;
        self.0 = tail;
        Some(*head)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u32()?).ok()?;
        self.take(n)
    }
}

/// The entry in `bytes`, or `None` when it is not a well-formed entry of
/// this format with a matching checksum and a PNG payload.
pub(super) fn decode(bytes: &[u8]) -> Option<Decoded<'_>> {
    let (body, sum) = bytes.split_last_chunk::<8>()?;
    let mut r = Reader(body);
    if r.take(MAGIC.len())? != MAGIC || r.u32()? != FORMAT_VERSION {
        return None;
    }
    if u64::from_le_bytes(*sum) != checksum(body) {
        return None;
    }
    let producer = r.bytes()?;
    let key = r.bytes()?;
    let size = r.u32()?;
    let stamp = Stamp {
        len: r.u64()?,
        modified_ns: r.u64()?,
    };
    let png = r.bytes()?;
    if !r.0.is_empty() || !png.starts_with(&PNG_SIGNATURE) {
        return None;
    }
    Some(Decoded {
        producer,
        key,
        size,
        stamp,
        png,
    })
}
