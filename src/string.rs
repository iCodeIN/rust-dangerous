use core::str;

use unicode_width::UnicodeWidthChar;

// Source: <rust-source>/core/str/mod.rs
// https://tools.ietf.org/html/rfc3629
static UTF8_CHAR_LENGTH: [u8; 256] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x1F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x3F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x5F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x7F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0x9F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0xBF
    0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, // 0xDF
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, // 0xEF
    4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xFF
];

/// Mask of the value bits of a continuation byte.
const CONT_MASK: u8 = 0b0011_1111;
/// Value of the tag bits (tag mask is !`CONT_MASK`) of a continuation byte.
const TAG_CONT_U8: u8 = 0b1000_0000;

/// Checks whether the byte is a UTF-8 continuation byte (i.e., starts with the
/// bits `10`).
#[inline]
fn utf8_is_cont_byte(byte: u8) -> bool {
    (byte & !CONT_MASK) == TAG_CONT_U8
}

/// Given a first byte, determines how many bytes are in this UTF-8 character.
#[inline]
pub(crate) fn utf8_char_len(b: u8) -> usize {
    UTF8_CHAR_LENGTH[b as usize] as usize
}

#[inline]
pub(crate) fn utf8_char_display_width(c: char, cjk: bool) -> usize {
    if c == '\0' {
        return "\\u{0}".len();
    }
    let width = if cjk { c.width_cjk() } else { c.width() };
    match width {
        Some(width) => width,
        None => "\\u{}".len() + count_digits(c as u32),
    }
}

pub(crate) fn count_digits(mut num: u32) -> usize {
    let mut count = 1;
    while num > 9 {
        count += 1;
        num /= 10;
    }
    count
}

#[derive(Clone)]
pub(crate) struct CharIter<'i> {
    forward: usize,
    backward: usize,
    bytes: &'i [u8],
}

impl<'i> CharIter<'i> {
    pub(crate) fn new(bytes: &'i [u8]) -> Self {
        Self {
            bytes,
            forward: 0,
            backward: bytes.len(),
        }
    }

    pub(crate) fn as_slice(&self) -> &'i [u8] {
        &self.bytes[self.forward..self.backward]
    }

    fn head(&self) -> &'i [u8] {
        &self.bytes[self.forward..]
    }

    fn tail(&self) -> &'i [u8] {
        &self.bytes[..self.backward]
    }
}

impl<'i> Iterator for CharIter<'i> {
    type Item = Result<char, InvalidChar>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.forward == self.backward {
            None
        } else {
            let result = first_codepoint(self.head()).and_then(|c| {
                let forward = self.forward.saturating_add(c.len_utf8());
                if forward > self.backward {
                    self.forward = self.backward;
                    Err(InvalidChar(()))
                } else {
                    self.forward = forward;
                    Ok(c)
                }
            });
            Some(result)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.backward - self.forward;
        (remaining, Some(remaining))
    }
}

impl<'i> DoubleEndedIterator for CharIter<'i> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.forward == self.backward {
            None
        } else {
            let result = last_codepoint(self.tail()).and_then(|c| {
                let backward = self.backward.saturating_sub(c.len_utf8());
                if backward < self.forward {
                    self.backward = self.forward;
                    Err(InvalidChar(()))
                } else {
                    self.backward = backward;
                    Ok(c)
                }
            });
            Some(result)
        }
    }
}

#[derive(Debug)]
pub(crate) struct InvalidChar(());

#[inline(always)]
fn first_codepoint(bytes: &[u8]) -> Result<char, InvalidChar> {
    if let Some(first_byte) = bytes.first() {
        let len = utf8_char_len(*first_byte);
        if bytes.len() >= len {
            return parse_char(&bytes[..len]);
        }
    }
    Err(InvalidChar(()))
}

#[inline(always)]
fn last_codepoint(bytes: &[u8]) -> Result<char, InvalidChar> {
    if bytes.is_empty() {
        return Err(InvalidChar(()));
    }
    for (i, byte) in (1..=4).zip(bytes.iter().rev().copied()) {
        if !utf8_is_cont_byte(byte) && utf8_char_len(byte) == i {
            let last_index = bytes.len() - i;
            return parse_char(&bytes[last_index..]);
        }
    }
    Err(InvalidChar(()))
}

#[inline(always)]
fn parse_char(bytes: &[u8]) -> Result<char, InvalidChar> {
    if let Ok(s) = str::from_utf8(bytes) {
        if let Some(c) = s.chars().next() {
            return Ok(c);
        }
    }
    Err(InvalidChar(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_iter() {
        let mut char_iter = CharIter::new("\u{10348}a\u{10347}".as_bytes());
        assert_eq!(char_iter.next().unwrap().unwrap(), '\u{10348}');
        assert_eq!(char_iter.next_back().unwrap().unwrap(), '\u{10347}');
        assert_eq!(char_iter.next().unwrap().unwrap(), 'a');
    }

    #[test]
    fn test_last_codepoint() {
        assert!(last_codepoint(b"").is_err());
        assert!(last_codepoint(b"\xFF").is_err());
        assert!(last_codepoint(b"a\xFF").is_err());
        assert_eq!(last_codepoint(b"a").unwrap(), 'a');
        assert_eq!(last_codepoint(b"ab").unwrap(), 'b');
        assert_eq!(
            last_codepoint("a\u{10348}".as_bytes()).unwrap(),
            '\u{10348}'
        );
        assert_eq!(last_codepoint("\u{10348}".as_bytes()).unwrap(), '\u{10348}');
    }

    #[test]
    fn test_first_codepoint() {
        assert!(first_codepoint(b"").is_err());
        assert!(first_codepoint(b"\xFF").is_err());
        assert!(first_codepoint(b"\xFFa").is_err());
        assert_eq!(first_codepoint(b"a").unwrap(), 'a');
        assert_eq!(first_codepoint(b"ab").unwrap(), 'a');
        assert_eq!(
            first_codepoint("\u{10348}a".as_bytes()).unwrap(),
            '\u{10348}'
        );
        assert_eq!(
            first_codepoint("\u{10348}".as_bytes()).unwrap(),
            '\u{10348}'
        );
    }
}