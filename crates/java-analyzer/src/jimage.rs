//! Minimal, zero-dependency, panic-free reader for OpenJDK `lib/modules` (`jimage`) archives.
//!
//! Supports jimage v1.0 (JDK 9–25) and v1.1 (JDK 26–27).
//!
//! TODO: This is a temporary implementation meant to be replaced with a proper implementation
//! in `ristretto_jimage` if and when that becomes available (supporting jimage v1.1 for JDK 26–27
//! JEP 493 runtimes, handling compressed entries without panicking, and matching our workspace
//! `rustc` MSRV — `ristretto_jimage` 0.31.0 hardcodes `SUPPORTED_MINOR_VERSION = 0` and rejects
//! JDK 27's `lib/modules` with `Version not supported: 1.1`, while 0.32+ requires `rustc` 1.97+).

use std::{fmt, fs, io, path::Path};

const HEADER_SIZE: usize = 28;
const IMAGE_MAGIC_BE: [u8; 4] = [0xCA, 0xFE, 0xDA, 0xDA];
const IMAGE_MAGIC_LE: [u8; 4] = [0xDA, 0xDA, 0xFE, 0xCA];

const ATTR_END: u8 = 0;
const ATTR_MODULE: u8 = 1;
const ATTR_PARENT: u8 = 2;
const ATTR_BASE: u8 = 3;
const ATTR_EXTENSION: u8 = 4;
const ATTR_OFFSET: u8 = 5;
const ATTR_COMPRESSED: u8 = 6;
const ATTR_UNCOMPRESSED: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

#[derive(Debug)]
pub(crate) enum JimageError {
    Io(io::Error),
    Truncated,
    InvalidMagic([u8; 4]),
    UnsupportedVersion { major: u16, minor: u16 },
    InvalidStringOffset(usize),
    InvalidUtf8,
}

impl fmt::Display for JimageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Truncated => f.write_str("truncated or out-of-bounds jimage file"),
            Self::InvalidMagic(bytes) => write!(f, "invalid jimage magic bytes: {bytes:02x?}"),
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "unsupported jimage version: {major}.{minor}")
            }
            Self::InvalidStringOffset(offset) => {
                write!(f, "invalid jimage string table offset: {offset}")
            }
            Self::InvalidUtf8 => f.write_str("invalid UTF-8 in jimage string table"),
        }
    }
}

impl std::error::Error for JimageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for JimageError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Parsed OpenJDK `lib/modules` (`jimage`) container.
#[derive(Debug)]
pub(crate) struct Jimage {
    bytes: Vec<u8>,
    endian: Endian,
    table_length: usize,
    offsets_start: usize,
    locations_start: usize,
    strings_start: usize,
    data_start: usize,
}

/// Decoded resource entry borrowed from a [`Jimage`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct JimageEntry<'a> {
    pub module: &'a str,
    pub parent: &'a str,
    pub base: &'a str,
    pub extension: &'a str,
    data_offset: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    data_section: &'a [u8],
}

impl<'a> JimageEntry<'a> {
    /// Returns the canonical jimage path (`/<module>/<parent>/<base>.<extension>`).
    #[must_use]
    pub(crate) fn full_name(&self) -> String {
        let mut out = String::with_capacity(
            self.module.len() + self.parent.len() + self.base.len() + self.extension.len() + 4,
        );
        if !self.module.is_empty() {
            out.push('/');
            out.push_str(self.module);
            out.push('/');
        }
        if !self.parent.is_empty() {
            out.push_str(self.parent);
            out.push('/');
        }
        out.push_str(self.base);
        if !self.extension.is_empty() {
            out.push('.');
            out.push_str(self.extension);
        }
        out
    }

    /// Returns `Ok(Some(bytes))` for uncompressed entries, or `Ok(None)` if compressed.
    pub(crate) fn uncompressed_data(&self) -> Result<Option<&'a [u8]>, JimageError> {
        if self.compressed_size != 0 {
            tracing::debug!(
                entry = %self.full_name(),
                compressed_size = self.compressed_size,
                "skipping compressed jimage entry"
            );
            return Ok(None);
        }

        let start = usize::try_from(self.data_offset).map_err(|_| JimageError::Truncated)?;
        let len = usize::try_from(self.uncompressed_size).map_err(|_| JimageError::Truncated)?;
        let end = start.checked_add(len).ok_or(JimageError::Truncated)?;
        let slice = self
            .data_section
            .get(start..end)
            .ok_or(JimageError::Truncated)?;
        Ok(Some(slice))
    }
}

impl Jimage {
    /// Reads and validates a `jimage` file from disk.
    pub(crate) fn from_file(path: &Path) -> Result<Self, JimageError> {
        let bytes = fs::read(path)?;
        Self::from_bytes(bytes)
    }

    /// Validates an in-memory `jimage` byte buffer.
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Result<Self, JimageError> {
        let magic_slice = bytes.get(..4).ok_or(JimageError::Truncated)?;
        let magic: [u8; 4] = magic_slice
            .try_into()
            .map_err(|_| JimageError::Truncated)?;
        let endian = if magic == IMAGE_MAGIC_BE {
            Endian::Big
        } else if magic == IMAGE_MAGIC_LE {
            Endian::Little
        } else {
            return Err(JimageError::InvalidMagic(magic));
        };

        if bytes.len() < HEADER_SIZE {
            return Err(JimageError::Truncated);
        }

        let version = read_u32(&bytes, 4, endian)?;
        let major = u16::try_from(version >> 16).map_err(|_| JimageError::Truncated)?;
        let minor = u16::try_from(version & 0xFFFF).map_err(|_| JimageError::Truncated)?;
        if major != 1 || (minor != 0 && minor != 1) {
            return Err(JimageError::UnsupportedVersion { major, minor });
        }

        let _flags = read_u32(&bytes, 8, endian)?;
        let _resource_count = read_u32(&bytes, 12, endian)?;
        let table_length = usize::try_from(read_u32(&bytes, 16, endian)?)
            .map_err(|_| JimageError::Truncated)?;
        let locations_size = usize::try_from(read_u32(&bytes, 20, endian)?)
            .map_err(|_| JimageError::Truncated)?;
        let strings_size = usize::try_from(read_u32(&bytes, 24, endian)?)
            .map_err(|_| JimageError::Truncated)?;

        let table_bytes = table_length.checked_mul(4).ok_or(JimageError::Truncated)?;
        let offsets_start = HEADER_SIZE
            .checked_add(table_bytes)
            .ok_or(JimageError::Truncated)?;
        let locations_start = offsets_start
            .checked_add(table_bytes)
            .ok_or(JimageError::Truncated)?;
        let strings_start = locations_start
            .checked_add(locations_size)
            .ok_or(JimageError::Truncated)?;
        let data_start = strings_start
            .checked_add(strings_size)
            .ok_or(JimageError::Truncated)?;

        if data_start > bytes.len() {
            return Err(JimageError::Truncated);
        }

        Ok(Self {
            bytes,
            endian,
            table_length,
            offsets_start,
            locations_start,
            strings_start,
            data_start,
        })
    }

    /// Iterates over every location entry referenced by the `offsets_table`.
    pub(crate) fn entries(&self) -> JimageIter<'_> {
        let offsets_bytes = self
            .bytes
            .get(self.offsets_start..self.locations_start)
            .unwrap_or(&[]);
        let locations = self
            .bytes
            .get(self.locations_start..self.strings_start)
            .unwrap_or(&[]);
        let strings = self
            .bytes
            .get(self.strings_start..self.data_start)
            .unwrap_or(&[]);
        let data_section = self.bytes.get(self.data_start..).unwrap_or(&[]);

        JimageIter {
            endian: self.endian,
            offsets_bytes,
            locations,
            strings,
            data_section,
            index: 0,
            table_length: self.table_length,
        }
    }
}

pub(crate) struct JimageIter<'a> {
    endian: Endian,
    offsets_bytes: &'a [u8],
    locations: &'a [u8],
    strings: &'a [u8],
    data_section: &'a [u8],
    index: usize,
    table_length: usize,
}

impl<'a> Iterator for JimageIter<'a> {
    type Item = Result<JimageEntry<'a>, JimageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.table_length {
            return None;
        }
        let byte_pos = self.index.checked_mul(4)?;
        self.index += 1;

        let offset = match read_u32(self.offsets_bytes, byte_pos, self.endian) {
            Ok(v) => match usize::try_from(v) {
                Ok(off) => off,
                Err(_) => return Some(Err(JimageError::Truncated)),
            },
            Err(err) => return Some(Err(err)),
        };

        Some(decode_entry(
            self.locations,
            self.strings,
            self.data_section,
            offset,
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.table_length.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

fn decode_entry<'a>(
    locations: &'a [u8],
    strings: &'a [u8],
    data_section: &'a [u8],
    start_offset: usize,
) -> Result<JimageEntry<'a>, JimageError> {
    let mut cursor = start_offset;
    let mut module_off = 0usize;
    let mut parent_off = 0usize;
    let mut base_off = 0usize;
    let mut extension_off = 0usize;
    let mut data_offset = 0u64;
    let mut compressed_size = 0u64;
    let mut uncompressed_size = 0u64;

    loop {
        let tag = *locations.get(cursor).ok_or(JimageError::Truncated)?;
        cursor += 1;
        let kind = tag >> 3;
        if kind == ATTR_END {
            break;
        }

        let len = usize::from(tag & 0x07) + 1;
        let end = cursor.checked_add(len).ok_or(JimageError::Truncated)?;
        let val_bytes = locations.get(cursor..end).ok_or(JimageError::Truncated)?;
        cursor = end;

        let mut value = 0u64;
        for &b in val_bytes {
            value = (value << 8) | u64::from(b);
        }

        match kind {
            ATTR_MODULE => {
                module_off = usize::try_from(value).map_err(|_| JimageError::Truncated)?;
            }
            ATTR_PARENT => {
                parent_off = usize::try_from(value).map_err(|_| JimageError::Truncated)?;
            }
            ATTR_BASE => {
                base_off = usize::try_from(value).map_err(|_| JimageError::Truncated)?;
            }
            ATTR_EXTENSION => {
                extension_off = usize::try_from(value).map_err(|_| JimageError::Truncated)?;
            }
            ATTR_OFFSET => data_offset = value,
            ATTR_COMPRESSED => compressed_size = value,
            ATTR_UNCOMPRESSED => uncompressed_size = value,
            // Tolerate newer/higher attribute kinds (e.g. v1.1 preview flags) by skipping `len` bytes.
            _ => {}
        }
    }

    Ok(JimageEntry {
        module: read_cstr(strings, module_off)?,
        parent: read_cstr(strings, parent_off)?,
        base: read_cstr(strings, base_off)?,
        extension: read_cstr(strings, extension_off)?,
        data_offset,
        compressed_size,
        uncompressed_size,
        data_section,
    })
}

fn read_cstr(strings: &[u8], offset: usize) -> Result<&str, JimageError> {
    if offset == 0 {
        return Ok("");
    }
    let tail = strings
        .get(offset..)
        .ok_or(JimageError::InvalidStringOffset(offset))?;
    let nul_pos = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or(JimageError::InvalidStringOffset(offset))?;
    let raw = tail
        .get(..nul_pos)
        .ok_or(JimageError::InvalidStringOffset(offset))?;
    std::str::from_utf8(raw).map_err(|_| JimageError::InvalidUtf8)
}

fn read_u32(bytes: &[u8], offset: usize, endian: Endian) -> Result<u32, JimageError> {
    let end = offset.checked_add(4).ok_or(JimageError::Truncated)?;
    let raw = bytes.get(offset..end).ok_or(JimageError::Truncated)?;
    let arr: [u8; 4] = raw.try_into().map_err(|_| JimageError::Truncated)?;
    Ok(match endian {
        Endian::Little => u32::from_le_bytes(arr),
        Endian::Big => u32::from_be_bytes(arr),
    })
}
