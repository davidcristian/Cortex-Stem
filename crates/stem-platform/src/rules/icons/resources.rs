//! PE icon resources: which paths are PE images, and the `RT_GROUP_ICON`
//! directory with the rule that picks the image to load from it.

/// Whether `path` names a PE image whose icon resources can be read
/// directly: the file name ends in `.exe`, `.dll` or `.scr` (any case).
/// Explicit string handling, so the answer is the same on every host
/// (`std::path` treats `\` as a separator only on Windows).
pub fn is_pe_image_path(path: &str) -> bool {
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "exe" | "dll" | "scr"
        )
}

/// Byte length of the `GRPICONDIR` header (`idReserved`, `idType`,
/// `idCount`, each `u16`).
pub const GROUP_ICON_HEADER_LEN: usize = 6;
/// Byte length of one `GRPICONDIRENTRY`.
pub const GROUP_ICON_ENTRY_LEN: usize = 14;
/// `idType` of an icon directory (2 is a cursor directory).
const GROUP_ICON_TYPE: u16 = 1;

/// One image of an `RT_GROUP_ICON` directory: the dimensions it declares
/// (0 in the resource means 256) and the id of the `RT_ICON` resource that
/// holds its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupIconEntry {
    pub width: u32,
    pub height: u32,
    /// `wBitCount`: bits per pixel of the image (a PNG entry says 32).
    pub bit_count: u16,
    /// `dwBytesInRes`: the size of the `RT_ICON` resource.
    pub bytes_in_res: u32,
    /// `nId`: the `RT_ICON` resource id.
    pub id: u16,
}

impl GroupIconEntry {
    /// The longer side.
    pub fn edge(&self) -> u32 {
        self.width.max(self.height)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GroupIconError {
    #[error("icon group directory is truncated ({len} bytes, {needed} needed)")]
    Truncated { len: usize, needed: usize },
    #[error("icon group directory has type {0}, expected {GROUP_ICON_TYPE}")]
    BadType(u16),
    #[error("icon group directory lists no images")]
    Empty,
}

/// Parse an `RT_GROUP_ICON` resource (`GRPICONDIR`: `idReserved u16`,
/// `idType u16 = 1`, `idCount u16`, then `idCount` entries of
/// `bWidth u8, bHeight u8, bColorCount u8, bReserved u8, wPlanes u16,
/// wBitCount u16, dwBytesInRes u32, nId u16`, all little-endian). A width
/// or height of 0 means 256. Trailing bytes after the last entry are
/// ignored; `idReserved` is not checked (tools do write junk there).
pub fn parse_group_icon_dir(bytes: &[u8]) -> Result<Vec<GroupIconEntry>, GroupIconError> {
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if bytes.len() < GROUP_ICON_HEADER_LEN {
        return Err(GroupIconError::Truncated {
            len: bytes.len(),
            needed: GROUP_ICON_HEADER_LEN,
        });
    }
    let kind = u16_at(2);
    if kind != GROUP_ICON_TYPE {
        return Err(GroupIconError::BadType(kind));
    }
    let count = usize::from(u16_at(4));
    if count == 0 {
        return Err(GroupIconError::Empty);
    }
    let needed = GROUP_ICON_HEADER_LEN + count * GROUP_ICON_ENTRY_LEN;
    if bytes.len() < needed {
        return Err(GroupIconError::Truncated {
            len: bytes.len(),
            needed,
        });
    }
    let edge = |declared: u8| {
        if declared == 0 {
            256
        } else {
            u32::from(declared)
        }
    };
    Ok((0..count)
        .map(|i| {
            let at = GROUP_ICON_HEADER_LEN + i * GROUP_ICON_ENTRY_LEN;
            GroupIconEntry {
                width: edge(bytes[at]),
                height: edge(bytes[at + 1]),
                bit_count: u16_at(at + 6),
                bytes_in_res: u32_at(at + 8),
                id: u16_at(at + 12),
            }
        })
        .collect())
}

/// The image to load from an icon group when the request is at most
/// `max_edge` pixels: the largest edge that is `<= max_edge`, ties broken by
/// the highest bit depth, then the largest resource, then the earliest
/// entry. `None` when every entry is larger than `max_edge`.
pub fn select_group_icon_entry(
    entries: &[GroupIconEntry],
    max_edge: u32,
) -> Option<GroupIconEntry> {
    entries
        .iter()
        .rev()
        .filter(|entry| entry.edge() <= max_edge)
        .max_by_key(|entry| (entry.edge(), entry.bit_count, entry.bytes_in_res))
        .copied()
}
