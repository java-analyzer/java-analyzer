use lsp_types::{Position, Range};

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
}
