use super::Format;

pub(super) fn get_text_for_format(fmt: Format, data: &[u8]) -> String {
    match fmt {
        Format::Utf8 => String::from_utf8_lossy(data).into_owned(),
        Format::Ansi => decode_windows_1251(data),
        Format::Hex => to_hex_dump(data),
        Format::Image => String::new(),
    }
}

pub(super) fn to_hex_dump(bytes: &[u8]) -> String {
    let mut dump = String::new();
    for (offset, chunk) in bytes.chunks(16).enumerate() {
        dump.push_str(&format!("{:08x}:  ", offset * 16));
        for (i, byte) in chunk.iter().enumerate() {
            dump.push_str(&format!("{:02x} ", byte));
            if i == 7 {
                dump.push(' ');
            }
        }
        if chunk.len() < 16 {
            for i in chunk.len()..16 {
                dump.push_str("   ");
                if i == 7 {
                    dump.push(' ');
                }
            }
        }
        dump.push_str(" |");
        for byte in chunk {
            if *byte >= 32 && *byte <= 126 {
                dump.push(*byte as char);
            } else {
                dump.push('.');
            }
        }
        dump.push_str("|\n");
    }
    dump
}

pub(super) fn decode_windows_1251(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        if b <= 0x7F {
            s.push(b as char);
        } else {
            let c = match b {
                0x80 => 'Ђ',
                0x81 => 'Ѓ',
                0x82 => '‚',
                0x83 => 'ѓ',
                0x84 => '„',
                0x85 => '…',
                0x86 => '†',
                0x87 => '‡',
                0x88 => '€',
                0x89 => '‰',
                0x8A => 'Љ',
                0x8B => '‹',
                0x8C => 'Њ',
                0x8D => 'Ќ',
                0x8E => 'Ћ',
                0x8F => 'Џ',
                0x90 => 'ђ',
                0x91 => '\u{2018}',
                0x92 => '\u{2019}',
                0x93 => '\u{201C}',
                0x94 => '\u{201D}',
                0x95 => '•',
                0x96 => '–',
                0x97 => '—',
                0x98 => ' ',
                0x99 => '™',
                0x9A => 'љ',
                0x9B => '›',
                0x9C => 'њ',
                0x9D => 'ќ',
                0x9E => 'ћ',
                0x9F => 'џ',
                0xA0 => '\u{00A0}',
                0xA1 => 'Ў',
                0xA2 => 'ў',
                0xA3 => 'Ј',
                0xA4 => '¤',
                0xA5 => 'Ґ',
                0xA6 => '¦',
                0xA7 => '§',
                0xA8 => 'Ё',
                0xA9 => '©',
                0xAA => 'Є',
                0xAB => '«',
                0xAC => '¬',
                0xAD => '\u{00AD}',
                0xAE => '®',
                0xAF => 'Ї',
                0xB0 => '°',
                0xB1 => '±',
                0xB2 => 'І',
                0xB3 => 'і',
                0xB4 => 'ґ',
                0xB5 => 'µ',
                0xB6 => '¶',
                0xB7 => '·',
                0xB8 => 'ё',
                0xB9 => '№',
                0xBA => 'є',
                0xBB => '»',
                0xBC => 'ј',
                0xBD => 'Ѕ',
                0xBE => 'ѕ',
                0xBF => 'ї',
                0xC0..=0xFF => {
                    let code = 0x0410 + (b - 0xC0) as u32;
                    std::char::from_u32(code).unwrap_or('?')
                }
                _ => '?',
            };
            s.push(c);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Format;

    #[test]
    fn hex_dump_empty_bytes_returns_empty_string() {
        assert_eq!(to_hex_dump(&[]), "");
    }

    #[test]
    fn hex_dump_format_has_offset_and_ascii_sidebar() {
        let data = b"ABCD";
        let dump = to_hex_dump(data);
        assert!(dump.starts_with("00000000:"), "dump: {dump}");
        assert!(dump.contains("|ABCD|"), "dump: {dump}");
    }

    #[test]
    fn hex_dump_second_chunk_has_offset_16() {
        let data = vec![0u8; 20];
        let dump = to_hex_dump(&data);
        assert!(dump.contains("00000010:"), "second line: {dump}");
    }

    #[test]
    fn hex_dump_non_printable_shows_dot_in_sidebar() {
        let dump = to_hex_dump(&[0x01]);
        let pipe_section = dump.split('|').nth(1).unwrap();
        assert_eq!(pipe_section.chars().next().unwrap(), '.');
    }





    #[test]
    fn windows_1251_ascii_passthrough() {
        assert_eq!(decode_windows_1251(b"Hello, world!"), "Hello, world!");
    }

    #[test]
    fn windows_1251_decodes_capital_a() {
        assert_eq!(decode_windows_1251(&[0xC0]), "А");
    }

    #[test]
    fn windows_1251_decodes_full_cyrillic_range() {
        let bytes: Vec<u8> = (0xC0..=0xFF).collect();
        let s = decode_windows_1251(&bytes);
        assert!(s.starts_with('А'), "should start with А, got: {}", &s[..s.char_indices().nth(1).map(|(i,_)|i).unwrap_or(s.len())]);
        assert!(s.ends_with('я'));
    }

    #[test]
    fn encode_windows_1251_capital_a() {
        assert_eq!(encode_windows_1251("А"), vec![0xC0]);
    }

    #[test]
    fn encode_decode_roundtrip_cyrillic() {
        let text = "Привет мир";
        assert_eq!(decode_windows_1251(&encode_windows_1251(text)), text);
    }

    #[test]
    fn encode_windows_1251_unknown_char_becomes_question_mark() {
        let bytes = encode_windows_1251("中");
        assert_eq!(bytes, b"?");
    }

    #[test]
    fn get_text_format_hex_produces_hex_dump() {
        let result = get_text_for_format(Format::Hex, b"test");
        assert!(result.starts_with("00000000:"), "result: {result}");
    }

    #[test]
    fn get_text_format_utf8_returns_string() {
        assert_eq!(get_text_for_format(Format::Utf8, b"hello"), "hello");
    }

    #[test]
    fn get_text_format_image_returns_empty() {
        assert_eq!(get_text_for_format(Format::Image, b"anything"), "");
    }

    #[test]
    fn get_text_format_ansi_decodes_windows_1251() {
        let result = get_text_for_format(Format::Ansi, &[0xC0]);
        assert_eq!(result, "А");
    }
}

pub(super) fn encode_windows_1251(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(s.len());
    for c in s.chars() {
        let b = if c.is_ascii() {
            c as u8
        } else {
            match c {
                'Ђ' => 0x80,
                'Ѓ' => 0x81,
                '‚' => 0x82,
                'ѓ' => 0x83,
                '„' => 0x84,
                '…' => 0x85,
                '†' => 0x86,
                '‡' => 0x87,
                '€' => 0x88,
                '‰' => 0x89,
                'Љ' => 0x8A,
                '‹' => 0x8B,
                'Њ' => 0x8C,
                'Ќ' => 0x8D,
                'Ћ' => 0x8E,
                'Џ' => 0x8F,
                'ђ' => 0x90,
                '\u{2018}' => 0x91,
                '\u{2019}' => 0x92,
                '\u{201C}' => 0x93,
                '\u{201D}' => 0x94,
                '•' => 0x95,
                '–' => 0x96,
                '—' => 0x97,
                '™' => 0x99,
                'љ' => 0x9A,
                '›' => 0x9B,
                'њ' => 0x9C,
                'ќ' => 0x9D,
                'ћ' => 0x9E,
                'џ' => 0x9F,
                '\u{00A0}' => 0xA0,
                'Ў' => 0xA1,
                'ў' => 0xA2,
                'Ј' => 0xA3,
                '¤' => 0xA4,
                'Ґ' => 0xA5,
                '¦' => 0xA6,
                '§' => 0xA7,
                'Ё' => 0xA8,
                '©' => 0xA9,
                'Є' => 0xAA,
                '«' => 0xAB,
                '¬' => 0xAC,
                '\u{00AD}' => 0xAD,
                '®' => 0xAE,
                'Ї' => 0xAF,
                '°' => 0xB0,
                '±' => 0xB1,
                'І' => 0xB2,
                'і' => 0xB3,
                'ґ' => 0xB4,
                'µ' => 0xB5,
                '¶' => 0xB6,
                '·' => 0xB7,
                'ё' => 0xB8,
                '№' => 0xB9,
                'є' => 0xBA,
                '»' => 0xBB,
                'ј' => 0xBC,
                'Ѕ' => 0xBD,
                'ѕ' => 0xBE,
                'ї' => 0xBF,
                _ => {
                    let u = c as u32;
                    if u >= 0x0410 && u <= 0x044F {
                        (0xC0 + (u - 0x0410)) as u8
                    } else {
                        b'?'
                    }
                }
            }
        };
        bytes.push(b);
    }
    bytes
}
