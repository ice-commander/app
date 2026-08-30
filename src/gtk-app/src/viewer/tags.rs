#[derive(Default)]
pub(super) struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover: Option<Vec<u8>>,
}

impl Tags {
    pub fn headline(&self, file_name: &str) -> String {
        match (&self.artist, &self.title) {
            (Some(a), Some(t)) => format!("{a} — {t}"),
            (None, Some(t)) => t.clone(),
            _ => file_name.to_string(),
        }
    }
}

fn synchsafe(b: &[u8]) -> usize {
    b.iter().fold(0usize, |acc, byte| (acc << 7) | (*byte as usize & 0x7F))
}

fn plain_u32(b: &[u8]) -> usize {
    b.iter().fold(0usize, |acc, byte| (acc << 8) | *byte as usize)
}

fn decode_text(encoding: u8, data: &[u8]) -> String {
    let trim = |s: String| s.trim_end_matches('\0').to_string();
    match encoding {
        0 => trim(data.iter().map(|b| *b as char).collect()),
        3 => trim(String::from_utf8_lossy(data).into_owned()),
        1 | 2 => {
            let (body, big_endian) = match data {
                [0xFF, 0xFE, rest @ ..] => (rest, false),
                [0xFE, 0xFF, rest @ ..] => (rest, true),
                _ => (data, encoding == 2),
            };
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|c| {
                    if big_endian {
                        u16::from_be_bytes([c[0], c[1]])
                    } else {
                        u16::from_le_bytes([c[0], c[1]])
                    }
                })
                .collect();
            trim(String::from_utf16_lossy(&units))
        }
        _ => String::new(),
    }
}

fn parse_apic(data: &[u8]) -> Option<Vec<u8>> {
    let encoding = *data.first()?;
    let rest = &data[1..];
    let mime_end = rest.iter().position(|b| *b == 0)?;
    let after_mime = rest.get(mime_end + 2..)?; // skip the NUL and the picture type byte

    let description_end = if encoding == 1 || encoding == 2 {
        after_mime
            .chunks_exact(2)
            .position(|c| c == [0, 0])
            .map(|i| i * 2 + 2)?
    } else {
        after_mime.iter().position(|b| *b == 0)? + 1
    };
    let picture = after_mime.get(description_end..)?;
    if picture.is_empty() {
        None
    } else {
        Some(picture.to_vec())
    }
}

pub(super) fn read_id3(bytes: &[u8]) -> Tags {
    let mut tags = Tags::default();
    if bytes.len() < 10 || !bytes.starts_with(b"ID3") {
        return tags;
    }
    let major = bytes[3];
    if major < 3 {
        return tags; // v2.2 uses three-letter frame ids; rare enough to ignore
    }
    let tag_size = synchsafe(&bytes[6..10]);
    let end = (10 + tag_size).min(bytes.len());

    let mut pos = 10;
    while pos + 10 <= end {
        let id = &bytes[pos..pos + 4];
        if id == [0, 0, 0, 0] {
            break;
        }
        let size = if major >= 4 {
            synchsafe(&bytes[pos + 4..pos + 8])
        } else {
            plain_u32(&bytes[pos + 4..pos + 8])
        };
        let body_start = pos + 10;
        let body_end = body_start + size;
        if size == 0 || body_end > end {
            break;
        }
        let body = &bytes[body_start..body_end];

        match id {
            b"TIT2" | b"TPE1" | b"TALB" => {
                let text = decode_text(body[0], &body[1..]);
                if !text.is_empty() {
                    match id {
                        b"TIT2" => tags.title = Some(text),
                        b"TPE1" => tags.artist = Some(text),
                        _ => tags.album = Some(text),
                    }
                }
            }
            b"APIC" if tags.cover.is_none() => tags.cover = parse_apic(body),
            _ => {}
        }
        pos = body_end;
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_frame(id: &[u8; 4], encoding: u8, text: &[u8]) -> Vec<u8> {
        let mut frame = id.to_vec();
        let size = (text.len() + 1) as u32;
        frame.extend_from_slice(&size.to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.push(encoding);
        frame.extend_from_slice(text);
        frame
    }

    fn tag_with(frames: Vec<u8>, major: u8) -> Vec<u8> {
        let mut out = b"ID3".to_vec();
        out.extend_from_slice(&[major, 0, 0]);
        let size = frames.len();
        out.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        out.extend_from_slice(&frames);
        out
    }

    #[test]
    fn reads_title_and_artist() {
        let mut frames = text_frame(b"TIT2", 3, "Take Five".as_bytes());
        frames.extend(text_frame(b"TPE1", 3, "Dave Brubeck".as_bytes()));
        let tags = read_id3(&tag_with(frames, 3));
        assert_eq!(tags.title.as_deref(), Some("Take Five"));
        assert_eq!(tags.artist.as_deref(), Some("Dave Brubeck"));
        assert_eq!(tags.headline("x.mp3"), "Dave Brubeck — Take Five");
    }

    #[test]
    fn utf16_with_bom_is_decoded() {
        let mut text = vec![0xFF, 0xFE];
        for unit in "Пример".encode_utf16() {
            text.extend_from_slice(&unit.to_le_bytes());
        }
        let tags = read_id3(&tag_with(text_frame(b"TIT2", 1, &text), 3));
        assert_eq!(tags.title.as_deref(), Some("Пример"));
    }

    #[test]
    fn a_file_without_a_tag_yields_the_file_name() {
        let tags = read_id3(b"\xff\xfb\x90\x00 raw mp3 frames");
        assert!(tags.title.is_none());
        assert_eq!(tags.headline("song.mp3"), "song.mp3");
    }

    #[test]
    fn cover_art_is_extracted() {
        let mut body = vec![0u8];
        body.extend_from_slice(b"image/png\0");
        body.push(3); // front cover
        body.extend_from_slice(b"cover\0");
        body.extend_from_slice(b"\x89PNG\r\n\x1a\n");

        let mut frame = b"APIC".to_vec();
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&body);

        let tags = read_id3(&tag_with(frame, 3));
        assert_eq!(tags.cover.as_deref(), Some(&b"\x89PNG\r\n\x1a\n"[..]));
    }

    #[test]
    fn a_truncated_frame_does_not_panic() {
        let mut tag = tag_with(text_frame(b"TIT2", 3, b"whatever"), 4);
        tag.truncate(tag.len() - 4);
        let _ = read_id3(&tag);
    }
}
