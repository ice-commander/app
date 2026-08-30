pub(super) fn extract_raw_thumbnail_from_bytes(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    let mut cursor = Cursor::new(data);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    let fields: Vec<_> = exif.fields().collect();

    let mut candidates = Vec::new();

    for field in &fields {
        let tag_num = field.tag.number();
        if tag_num == 0x0201 { // JPEGInterchangeFormat
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields.iter()
                    .find(|f| f.tag.number() == 0x0202 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
        if tag_num == 0x0111 { // StripOffsets
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields.iter()
                    .find(|f| f.tag.number() == 0x0117 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
        if tag_num == 0x0144 { // TileOffsets
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields.iter()
                    .find(|f| f.tag.number() == 0x0145 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
    }

    for (offset, length_opt) in candidates {
        if cursor.seek(SeekFrom::Start(offset as u64)).is_ok() {
            let mut magic = [0u8; 3];
            if cursor.read_exact(&mut magic).is_ok() {
                if magic[0] == 0xFF && magic[1] == 0xD8 && magic[2] == 0xFF {
                    if let Some(length) = length_opt {
                        cursor.seek(SeekFrom::Start(offset as u64)).ok()?;
                        let mut buf = vec![0u8; length as usize];
                        if cursor.read_exact(&mut buf).is_ok() {
                            return Some(buf);
                        }
                    }
                }
            }
        }
    }

    None
}
