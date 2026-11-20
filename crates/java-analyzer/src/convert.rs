use std::path::{Component, Path, PathBuf, Prefix};

use lsp_types::{Position, Range, Uri};

/// Converts an absolute filesystem [`Path`] into a `file://` [`Uri`].
///
/// Returns `None` if `path` is relative, uses an unsupported Windows prefix, or cannot be
/// represented as a valid URI.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    if !path.is_absolute() {
        return None;
    }

    let mut out = String::from("file://");
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    out.push('/');
                    out.push(drive as char);
                    out.push(':');
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    percent_encode_bytes(os_str_bytes(server)?, &mut out);
                    out.push('/');
                    percent_encode_bytes(os_str_bytes(share)?, &mut out);
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => return None,
            },
            Component::RootDir => {
                if out == "file://" || !out.ends_with('/') {
                    out.push('/');
                }
            }
            Component::CurDir => {
                if !out.ends_with('/') {
                    out.push('/');
                }
                out.push('.');
            }
            Component::ParentDir => {
                if !out.ends_with('/') {
                    out.push('/');
                }
                out.push_str("..");
            }
            Component::Normal(seg) => {
                if !out.ends_with('/') {
                    out.push('/');
                }
                percent_encode_bytes(os_str_bytes(seg)?, &mut out);
            }
        }
    }

    out.parse::<Uri>().ok()
}

/// Converts a `file://` [`Uri`] back into a local filesystem [`PathBuf`].
///
/// Returns `None` if `uri` is not a `file` URI, specifies a non-local host (on non-Windows),
/// or contains invalid percent-encoding or NUL bytes.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    if !s.get(..5)?.eq_ignore_ascii_case("file:") {
        return None;
    }

    let after_scheme = &s[5..];
    let without_query_or_fragment = match after_scheme.find(['?', '#']) {
        Some(idx) => &after_scheme[..idx],
        None => after_scheme,
    };

    if let Some(after_slashes) = without_query_or_fragment.strip_prefix("//") {
        let slash_idx = after_slashes.find('/')?;
        let authority = &after_slashes[..slash_idx];
        let raw_path = &after_slashes[slash_idx..];

        #[cfg(windows)]
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            let server = String::from_utf8(percent_decode_bytes(authority)?).ok()?;
            let path = String::from_utf8(percent_decode_bytes(raw_path)?).ok()?;
            return Some(PathBuf::from(format!(
                "\\\\{}{}",
                server,
                path.replace('/', "\\")
            )));
        }

        #[cfg(not(windows))]
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return None;
        }

        let decoded = percent_decode_bytes(raw_path)?;
        bytes_to_path_buf(decoded)
    } else if without_query_or_fragment.starts_with('/') {
        let decoded = percent_decode_bytes(without_query_or_fragment)?;
        bytes_to_path_buf(decoded)
    } else {
        None
    }
}

fn os_str_bytes(s: &std::ffi::OsStr) -> Option<&[u8]> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(s.as_bytes())
    }
    #[cfg(not(unix))]
    {
        s.to_str().map(str::as_bytes)
    }
}

#[cfg(unix)]
fn bytes_to_path_buf(bytes: Vec<u8>) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn bytes_to_path_buf(bytes: Vec<u8>) -> Option<PathBuf> {
    let s = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    {
        let mut chars = s.chars();
        if matches!(
            (chars.next(), chars.next(), chars.next(), chars.next()),
            (Some('/'), Some(d), Some(':'), None | Some('/')) if d.is_ascii_alphabetic()
        ) {
            let trimmed = &s[1..];
            let win = if trimmed.len() == 2 {
                format!("{trimmed}\\")
            } else {
                trimmed.replace('/', "\\")
            };
            return Some(PathBuf::from(win));
        }
    }
    Some(PathBuf::from(s))
}

fn percent_encode_bytes(bytes: &[u8], out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in bytes {
        if matches!(
            b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
        ) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
    }
}

fn percent_decode_bytes(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hi = hex_val(*bytes.get(i + 1)?)?;
            let lo = hex_val(*bytes.get(i + 2)?)?;
            let decoded = (hi << 4) | lo;
            if decoded == 0 {
                return None;
            }
            out.push(decoded);
            i += 3;
        } else {
            if b == 0 {
                return None;
            }
            out.push(b);
            i += 1;
        }
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Converts an LSP position (line + UTF-16 code unit column) into a byte offset in `text`.
///
/// Returns `None` if the line is out of range. A column past the end of the line is clamped
/// to the end of that line, as the LSP specification requires.
pub fn position_to_offset(text: &str, position: Position) -> Option<u32> {
    let mut line_start = 0usize;
    for _ in 0..position.line {
        let newline = text[line_start..].find('\n')?;
        line_start += newline + 1;
    }

    let line = &text[line_start..];
    let line_end = line.find('\n').unwrap_or(line.len());
    let line = &line[..line_end];

    let mut utf16_col = 0u32;
    let mut byte_col = line.len();
    for (idx, ch) in line.char_indices() {
        if utf16_col >= position.character {
            byte_col = idx;
            break;
        }
        utf16_col += ch.len_utf16() as u32;
    }

    u32::try_from(line_start + byte_col).ok()
}

/// Converts a byte offset in `text` into an LSP position (line + UTF-16 code unit column).
///
/// An offset past the end of `text` is clamped to the end; an offset inside a multi-byte
/// character is rounded down to the start of that character.
pub fn offset_to_position(text: &str, offset: u32) -> Position {
    let mut offset = (offset as usize).min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = before[line_start..].encode_utf16().count();
    Position {
        line: line as u32,
        character: character as u32,
    }
}

/// Converts a byte range in `text` into an LSP range.
pub fn text_range_to_range(text: &str, start: u32, end: u32) -> Range {
    Range {
        start: offset_to_position(text, start),
        end: offset_to_position(text, end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn first_line() {
        assert_eq!(position_to_offset("class Main {}", pos(0, 6)), Some(6));
    }

    #[test]
    fn later_line() {
        assert_eq!(position_to_offset("class A {\n}\n", pos(1, 0)), Some(10));
    }

    #[test]
    fn column_past_end_is_clamped_to_line_end() {
        assert_eq!(position_to_offset("ab\ncd", pos(0, 99)), Some(2));
    }

    #[test]
    fn line_out_of_range() {
        assert_eq!(position_to_offset("ab", pos(3, 0)), None);
    }

    #[test]
    fn offset_to_position_on_first_and_later_lines() {
        assert_eq!(offset_to_position("class A {\n}\n", 6), pos(0, 6));
        assert_eq!(offset_to_position("class A {\n}\n", 10), pos(1, 0));
        assert_eq!(offset_to_position("class A {\n}\n", 11), pos(1, 1));
    }

    #[test]
    fn offset_to_position_counts_utf16_units() {
        let text = "é😀x";
        assert_eq!(offset_to_position(text, 2), pos(0, 1));
        assert_eq!(offset_to_position(text, 6), pos(0, 3));
        assert_eq!(offset_to_position(text, 7), pos(0, 4));
    }

    #[test]
    fn offset_to_position_clamps_and_rounds() {
        assert_eq!(offset_to_position("ab", 99), pos(0, 2));
        // Offset 1 is inside the 2-byte 'é'.
        assert_eq!(offset_to_position("éx", 1), pos(0, 0));
    }

    #[test]
    fn offset_and_position_round_trip() {
        let text = "class Ä {\n  😀 y;\n}\n";
        for (offset, _) in text.char_indices() {
            let position = offset_to_position(text, offset as u32);
            assert_eq!(
                position_to_offset(text, position),
                Some(offset as u32),
                "{offset}"
            );
        }
    }

    #[test]
    fn utf16_columns_with_multibyte_chars() {
        // 'é' is 2 bytes / 1 UTF-16 unit; '😀' is 4 bytes / 2 UTF-16 units.
        let text = "é😀x";
        assert_eq!(position_to_offset(text, pos(0, 1)), Some(2));
        assert_eq!(position_to_offset(text, pos(0, 3)), Some(6));
    }

    #[test]
    fn path_to_uri_rejects_relative_path() {
        assert_eq!(path_to_uri(Path::new("src/Main.java")), None);
        assert_eq!(path_to_uri(Path::new("")), None);
    }

    #[cfg(unix)]
    #[test]
    fn path_to_uri_and_uri_to_path_round_trip_unix() {
        let cases = [
            "/",
            "/tmp/Main.java",
            "/tmp/my dir/Hello World.java",
            "/tmp/äöü/Café 😀.java",
            "/tmp/a#b?c%d/Main.java",
        ];
        for raw in cases {
            let path = PathBuf::from(raw);
            let uri = path_to_uri(&path).unwrap_or_else(|| panic!("failed for {raw}"));
            assert!(uri.as_str().starts_with("file:///"), "{uri:?}");
            assert_eq!(uri_to_path(&uri).as_deref(), Some(path.as_path()), "{raw}");
        }

        let space_uri = path_to_uri(Path::new("/tmp/my dir/Café.java")).unwrap();
        assert_eq!(space_uri.as_str(), "file:///tmp/my%20dir/Caf%C3%A9.java");
    }

    #[cfg(windows)]
    #[test]
    fn path_to_uri_and_uri_to_path_round_trip_windows() {
        let cases = [
            (r"C:\", "file:///C:/"),
            (r"C:\Users\my name\Main.java", "file:///C:/Users/my%20name/Main.java"),
            (r"C:\Users\Café 😀.java", "file:///C:/Users/Caf%C3%A9%20%F0%9F%98%80.java"),
            (r"\\server\share\dir\Main.java", "file://server/share/dir/Main.java"),
        ];
        for (raw, expected_uri) in cases {
            let path = PathBuf::from(raw);
            let uri = path_to_uri(&path).unwrap_or_else(|| panic!("failed for {raw}"));
            assert_eq!(uri.as_str(), expected_uri);
            assert_eq!(uri_to_path(&uri).as_deref(), Some(path.as_path()), "{raw}");
        }

        let encoded_colon: Uri = "file:///c%3A/Users/Main.java".parse().unwrap();
        assert_eq!(uri_to_path(&encoded_colon), Some(PathBuf::from(r"c:\Users\Main.java")));
    }

    #[cfg(unix)]
    #[test]
    fn uri_to_path_handles_localhost_single_slash_and_lowercase_hex() {
        let localhost: Uri = "file://localhost/tmp/Main.java".parse().unwrap();
        assert_eq!(uri_to_path(&localhost), Some(PathBuf::from("/tmp/Main.java")));

        let single_slash: Uri = "file:/tmp/Main.java".parse().unwrap();
        assert_eq!(uri_to_path(&single_slash), Some(PathBuf::from("/tmp/Main.java")));

        let lower_hex: Uri = "file:///tmp/a%20b/caf%c3%a9.java".parse().unwrap();
        assert_eq!(uri_to_path(&lower_hex), Some(PathBuf::from("/tmp/a b/café.java")));
    }

    #[test]
    fn uri_to_path_rejects_non_file_or_empty_path() {
        let http: Uri = "https://example.com/Main.java".parse().unwrap();
        assert_eq!(uri_to_path(&http), None);

        let untitled: Uri = "untitled:Main.java".parse().unwrap();
        assert_eq!(uri_to_path(&untitled), None);

        let no_path: Uri = "file://localhost".parse().unwrap();
        assert_eq!(uri_to_path(&no_path), None);

        #[cfg(not(windows))]
        {
            let remote: Uri = "file://example.com/tmp/Main.java".parse().unwrap();
            assert_eq!(uri_to_path(&remote), None);
        }
    }
}
