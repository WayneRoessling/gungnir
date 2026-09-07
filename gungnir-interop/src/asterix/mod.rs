//! ASTERIX framing shared by every category: data blocks, records, and the field
//! specification (FSPEC), per EUROCONTROL Specification for Surveillance Data
//! Exchange Part I edition 3.1 (pinned in `docs/design/external-standards.md` §1.6).
//!
//! A data block is `CAT (1 octet) | LEN (2 octets, big-endian, counts itself) |
//! records...`. A record is `FSPEC | data items in UAP order`. The FSPEC is a
//! sequence of octets whose bits 8 through 2 flag the presence of seven
//! consecutive field reference numbers (FRNs) and whose bit 1 (FX) says whether
//! another FSPEC octet follows.
//!
//! Nothing here interprets a data item. The category modules do that, using
//! [`Cursor`] so that every read is bounds-checked and every failure names the
//! byte offset it failed at.

pub mod cat034;
pub mod cat048;

use crate::InteropError;
use gungnir_model::SensorId;

/// One radar a category decoder accepts reports from, and where it stands in the
/// local frame. Shared by the Category 048 and 034 codecs: the same radar sends both.
///
/// The frame anchor comes from the caller, not from here: turning the registry's
/// geodetic position into local ENU needs `gungnir-geo`, which this crate may not
/// depend on (ARCHITECTURE.md §7). The ingest adapter that owns the feed supplies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarSite {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
    /// The antenna's position in the local ENU frame, metres.
    pub origin_enu_m: [f64; 3],
}

/// One ASTERIX data block: the category and the bytes after the length field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataBlock<'a> {
    pub category: u8,
    /// The records, without the three-octet block header.
    pub payload: &'a [u8],
    /// Offset of this block's first octet in the input, for error messages.
    pub offset: usize,
}

/// Split a byte string into its data blocks (Part I §5.1).
///
/// Empty input is zero blocks, not an error. A block whose declared length is
/// shorter than its own header or longer than the bytes left is a
/// [`InteropError::Malformed`] naming the offset, and nothing after it is
/// returned: a length field cannot be trusted past the first bad one.
pub fn data_blocks<'a>(
    codec: &'static str,
    bytes: &'a [u8],
) -> Result<Vec<DataBlock<'a>>, InteropError> {
    let mut blocks = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset + 3).ok_or_else(|| {
            malformed(codec, offset, "data block header is shorter than 3 octets")
        })?;
        let category = header[0];
        let len = usize::from(u16::from_be_bytes([header[1], header[2]]));
        if len < 3 {
            return Err(malformed(
                codec,
                offset,
                format!("data block length {len} is shorter than its own header"),
            ));
        }
        let payload = bytes.get(offset + 3..offset + len).ok_or_else(|| {
            malformed(
                codec,
                offset,
                format!(
                    "data block declares {len} octets but only {} remain",
                    bytes.len() - offset
                ),
            )
        })?;
        blocks.push(DataBlock {
            category,
            payload,
            offset,
        });
        offset += len;
    }
    Ok(blocks)
}

/// The presence flags of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fspec {
    /// `present[frn - 1]` for every FRN the FSPEC covered; FRNs past the end are absent.
    present: Vec<bool>,
}

impl Fspec {
    /// Read the FSPEC at the cursor (Part I §5.2.1).
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, InteropError> {
        let mut present = Vec::with_capacity(14);
        loop {
            let octet = cursor.u8("FSPEC octet")?;
            for bit in (1..=7).rev() {
                present.push(octet & (1 << bit) != 0);
            }
            if octet & 1 == 0 {
                break;
            }
        }
        Ok(Self { present })
    }

    /// Whether the item with this field reference number (1-based) is present.
    pub fn has(&self, frn: usize) -> bool {
        frn >= 1 && self.present.get(frn - 1).copied().unwrap_or(false)
    }

    /// FRNs flagged present beyond `max_frn`: the UAP has no item for them, so the
    /// record cannot be parsed past that point.
    pub fn beyond(&self, max_frn: usize) -> Option<usize> {
        self.present
            .iter()
            .enumerate()
            .find(|(i, p)| **p && *i >= max_frn)
            .map(|(i, _)| i + 1)
    }
}

/// A bounds-checked reader over one record.
#[derive(Debug)]
pub struct Cursor<'a> {
    codec: &'static str,
    data: &'a [u8],
    pos: usize,
    /// Offset of `data[0]` in the whole input, so errors cite absolute positions.
    base: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(codec: &'static str, data: &'a [u8], base: usize) -> Self {
        Self {
            codec,
            data,
            pos: 0,
            base,
        }
    }

    /// Absolute offset of the next unread octet.
    pub fn offset(&self) -> usize {
        self.base + self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Take `n` octets for the named item, or fail at the current offset.
    pub fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], InteropError> {
        let end = self.pos.checked_add(n).ok_or_else(|| {
            malformed(
                self.codec,
                self.offset(),
                format!("{what}: length overflows"),
            )
        })?;
        let slice = self.data.get(self.pos..end).ok_or_else(|| {
            malformed(
                self.codec,
                self.offset(),
                format!("{what}: needs {n} octets, {} remain", self.remaining()),
            )
        })?;
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self, what: &str) -> Result<u8, InteropError> {
        Ok(self.take(1, what)?[0])
    }

    pub fn u16(&mut self, what: &str) -> Result<u16, InteropError> {
        let b = self.take(2, what)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn i16(&mut self, what: &str) -> Result<i16, InteropError> {
        let b = self.take(2, what)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u24(&mut self, what: &str) -> Result<u32, InteropError> {
        let b = self.take(3, what)?;
        Ok(u32::from_be_bytes([0, b[0], b[1], b[2]]))
    }

    /// A variable-length item: one octet, then one more for each set FX bit
    /// (bit 1). Returns every octet including the first.
    pub fn extended(&mut self, what: &str) -> Result<Vec<u8>, InteropError> {
        let mut octets = Vec::with_capacity(2);
        loop {
            let octet = self.u8(what)?;
            octets.push(octet);
            if octet & 1 == 0 {
                return Ok(octets);
            }
        }
    }

    /// An explicit-length item (SP and RE fields): one length octet that counts
    /// itself, then the data. Returns the data without the length octet.
    pub fn explicit(&mut self, what: &str) -> Result<&'a [u8], InteropError> {
        let len = usize::from(self.u8(what)?);
        if len < 1 {
            return Err(malformed(
                self.codec,
                self.offset() - 1,
                format!("{what}: length indicator 0 cannot count itself"),
            ));
        }
        self.take(len - 1, what)
    }

    /// Build a malformed-input error at the current offset.
    pub fn error(&self, reason: impl Into<String>) -> InteropError {
        malformed(self.codec, self.offset(), reason)
    }
}

/// Sign-extend the low `bits` bits of `raw` (two's complement fields narrower
/// than their octets, such as the 14-bit flight level in I048/090).
pub(crate) fn sign_extend(raw: u16, bits: u32) -> i32 {
    let shift = 32 - bits;
    let widened = i32::from(raw) << shift;
    widened >> shift
}

pub(crate) fn malformed(
    codec: &'static str,
    offset: usize,
    reason: impl Into<String>,
) -> InteropError {
    InteropError::Malformed {
        codec,
        offset,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_blocks_and_refuses_bad_lengths() {
        let two = [0x30, 0x00, 0x04, 0xAA, 0x22, 0x00, 0x03];
        let blocks = data_blocks("t", &two).expect("two blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].category, 48);
        assert_eq!(blocks[0].payload, &[0xAA]);
        assert_eq!(blocks[1].category, 34);
        assert!(blocks[1].payload.is_empty());

        assert!(data_blocks("t", &[]).expect("empty is fine").is_empty());
        assert!(matches!(
            data_blocks("t", &[0x30, 0x00, 0x02]),
            Err(InteropError::Malformed { offset: 0, .. })
        ));
        assert!(matches!(
            data_blocks("t", &[0x30, 0x00, 0x09, 0x00]),
            Err(InteropError::Malformed { offset: 0, .. })
        ));
        assert!(matches!(
            data_blocks("t", &[0x30, 0x00, 0x03, 0x30]),
            Err(InteropError::Malformed { offset: 3, .. })
        ));
    }

    #[test]
    fn fspec_maps_bits_to_frns_across_extensions() {
        // 0b1000_0001 (FRN 1, FX) then 0b0000_0100 (FRN 13, no FX).
        let bytes = [0x81, 0x04, 0xFF];
        let mut cur = Cursor::new("t", &bytes, 0);
        let f = Fspec::read(&mut cur).expect("fspec");
        assert!(f.has(1));
        assert!(!f.has(2));
        assert!(f.has(13));
        assert!(!f.has(14));
        assert!(!f.has(0));
        assert!(!f.has(99));
        assert_eq!(cur.remaining(), 1);
        assert_eq!(f.beyond(12), Some(13));
        assert_eq!(f.beyond(13), None);
    }

    #[test]
    fn cursor_reads_are_bounds_checked() {
        let bytes = [0x01, 0x02, 0x03];
        let mut cur = Cursor::new("t", &bytes, 10);
        assert_eq!(cur.u16("x").expect("u16"), 0x0102);
        assert!(matches!(
            cur.u16("y"),
            Err(InteropError::Malformed { offset: 12, .. })
        ));
        assert_eq!(cur.u8("z").expect("u8"), 3);
        assert!(cur.is_empty());
    }

    #[test]
    fn explicit_length_counts_itself() {
        let mut cur = Cursor::new("t", &[0x03, 0xAA, 0xBB], 0);
        assert_eq!(cur.explicit("sp").expect("sp"), &[0xAA, 0xBB]);
        let mut zero = Cursor::new("t", &[0x00], 0);
        assert!(zero.explicit("sp").is_err());
    }

    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0x3FFF, 14), -1);
        assert_eq!(sign_extend(0x0190, 14), 400);
        assert_eq!(sign_extend(0x2000, 14), -8192);
    }
}
