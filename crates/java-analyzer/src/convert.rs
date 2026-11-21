use std::path::{Path, PathBuf};

use line_index::{LineIndex, TextSize, WideEncoding, WideLineCol};
use lsp_types::{Position, Range, Uri};

/// Converts an absolute filesystem [`Path`] into a `file://` [`Uri`].
///
/// Returns `None` if `path` is relative, uses an unsupported Windows prefix, or cannot be
/// represented as a valid URI.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let url = url::Url::from_file_path(path).ok()?;
    url.as_str().parse::<Uri>().ok()
}

/// Converts a `file://` [`Uri`] back into a local filesystem [`PathBuf`].
///
/// Returns `None` if `uri` is not a `file` URI, has an empty path, specifies a non-local host
/// (on non-Windows), or contains NUL bytes.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if uri.path().as_str().is_empty() {
        return None;
    }
    let url = url::Url::parse(uri.as_str()).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return None;
    }
    Some(path)
}

/// Converts an LSP position (line + UTF-16 code unit column) into a byte offset in `text`
/// using a precomputed [`LineIndex`].
///
/// Returns `None` if the line is out of range. A column past the end of the line is clamped
/// to the end of that line (before `\n`/`\r\n`), as the LSP specification requires.
pub fn position_to_offset_with_index(
    index: &LineIndex,
    text: &str,
    position: Position,
) -> Option<u32> {
    let line_range = index.line(position.line)?;
    let start = usize::from(line_range.start());
    let end = usize::from(line_range.end());
    let line_text = &text[start..end];
    let trimmed = line_text
        .strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(line_text);
    let line_end = line_range.start() + TextSize::of(trimmed);

    let wide_col = WideLineCol {
        line: position.line,
        col: position.character,
    };
    let offset = match index.to_utf8(WideEncoding::Utf16, wide_col) {
        Some(lc) => {
            let max_col = u32::from(line_end - line_range.start());
            let mut off = start + lc.col.min(max_col) as usize;
            while !text.is_char_boundary(off) {
                off -= 1;
            }
            off as u32
        }
        None => u32::from(line_end),
    };
    Some(offset)
}

/// Converts an LSP position (line + UTF-16 code unit column) into a byte offset in `text`.
///
/// Returns `None` if the line is out of range. A column past the end of the line is clamped
/// to the end of that line, as the LSP specification requires.
pub fn position_to_offset(text: &str, position: Position) -> Option<u32> {
    position_to_offset_with_index(&LineIndex::new(text), text, position)
}

/// Converts a byte offset in `text` into an LSP position (line + UTF-16 code unit column)
/// using a precomputed [`LineIndex`].
///
/// An offset past the end of `text` is clamped to the end; an offset inside a multi-byte
/// character is rounded down to the start of that character.
pub fn offset_to_position_with_index(index: &LineIndex, text: &str, offset: u32) -> Position {
    let mut off = (offset as usize).min(text.len()).min(usize::from(index.len()));
    while !text.is_char_boundary(off) {
        off -= 1;
    }
    let line_col = index.line_col(TextSize::from(off as u32));
    let wide = index
        .to_wide(WideEncoding::Utf16, line_col)
        .unwrap_or(WideLineCol {
            line: line_col.line,
            col: line_col.col,
        });
    Position {
        line: wide.line,
        character: wide.col,
    }
}

/// Converts a byte offset in `text` into an LSP position (line + UTF-16 code unit column).
///
/// An offset past the end of `text` is clamped to the end; an offset inside a multi-byte
/// character is rounded down to the start of that character.
pub fn offset_to_position(text: &str, offset: u32) -> Position {
    offset_to_position_with_index(&LineIndex::new(text), text, offset)
}

/// Converts a byte range in `text` into an LSP range using a precomputed [`LineIndex`].
pub fn text_range_to_range_with_index(
    index: &LineIndex,
    text: &str,
    start: u32,
    end: u32,
) -> Range {
    Range {
        start: offset_to_position_with_index(index, text, start),
        end: offset_to_position_with_index(index, text, end),
    }
}

/// Converts a byte range in `text` into an LSP range.
pub fn text_range_to_range(text: &str, start: u32, end: u32) -> Range {
    text_range_to_range_with_index(&LineIndex::new(text), text, start, end)
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

    #[test]
    fn uri_to_path_rejects_nul_bytes() {
        let nul: Uri = "file:///tmp/bad%00.java".parse().unwrap();
        assert_eq!(uri_to_path(&nul), None);
    }

    #[test]
    fn crlf_and_huge_columns_clamp_to_line_end() {
        assert_eq!(position_to_offset("ab\r\ncd", pos(0, 99)), Some(2));
        assert_eq!(position_to_offset("é😀x", pos(0, u32::MAX)), Some(7));
    }

    #[test]
    fn with_index_helpers_round_trip() {
        let text = "class A {\n  String s = \"😀\";\n}\n";
        let index = LineIndex::new(text);
        let offset = position_to_offset_with_index(&index, text, pos(1, 16)).unwrap();
        assert_eq!(offset, 28);
        assert_eq!(offset_to_position_with_index(&index, text, offset), pos(1, 16));
        // Mid-surrogate UTF-16 column rounds down to the start of the scalar value.
        assert_eq!(position_to_offset_with_index(&index, text, pos(1, 15)), Some(24));
        let range = text_range_to_range_with_index(&index, text, 10, offset);
        assert_eq!(range.start, pos(1, 0));
        assert_eq!(range.end, pos(1, 16));
    }
}
