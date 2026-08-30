use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn parse_archive_path(path: &str) -> Option<(PathBuf, String)> {
    let markers = [
        (".tar.gz", 7),
        (".tar.bz2", 8),
        (".tgz", 4),
        (".tbz2", 5),
        (".tbz", 4),
        (".tar", 4),
        (".zip", 4),
    ];
    for &(marker, len) in &markers {
        if let Some(idx) = path.to_lowercase().find(marker) {
            let end_idx = idx + len;
            if end_idx == path.len()
                || path.as_bytes()[end_idx] == b'/'
                || path.as_bytes()[end_idx] == b'\\'
            {
                let archive_path_str = &path[..end_idx];
                let mut internal_path_str = &path[end_idx..];
                while internal_path_str.starts_with('/') || internal_path_str.starts_with('\\') {
                    internal_path_str = &internal_path_str[1..];
                }
                return Some((
                    PathBuf::from(archive_path_str),
                    internal_path_str.to_string(),
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn non_archive_path_returns_none() {
        assert!(parse_archive_path("/home/user/readme.txt").is_none());
    }

    #[test]
    fn tar_gz_at_end_of_path_returns_empty_internal() {
        let (archive, internal) = parse_archive_path("/home/user/backup.tar.gz").unwrap();
        assert_eq!(archive, PathBuf::from("/home/user/backup.tar.gz"));
        assert_eq!(internal, "");
    }

    #[test]
    fn tar_gz_with_internal_path() {
        let (archive, internal) = parse_archive_path("/data/archive.tar.gz/dir/file.txt").unwrap();
        assert_eq!(archive, PathBuf::from("/data/archive.tar.gz"));
        assert_eq!(internal, "dir/file.txt");
    }

    #[test]
    fn tgz_extension_is_recognized() {
        let (archive, internal) = parse_archive_path("/data/archive.tgz/subdir").unwrap();
        assert_eq!(archive, PathBuf::from("/data/archive.tgz"));
        assert_eq!(internal, "subdir");
    }

    #[test]
    fn plain_tar_is_recognized() {
        let (archive, internal) = parse_archive_path("/data/archive.tar/readme.md").unwrap();
        assert_eq!(archive, PathBuf::from("/data/archive.tar"));
        assert_eq!(internal, "readme.md");
    }

    #[test]
    fn zip_extension_is_recognized() {
        let (archive, internal) = parse_archive_path("/data/project.zip/src/main.rs").unwrap();
        assert_eq!(archive, PathBuf::from("/data/project.zip"));
        assert_eq!(internal, "src/main.rs");
    }

    #[test]
    fn leading_slashes_in_internal_path_are_stripped() {
        let (_, internal) = parse_archive_path("/data/a.zip///subdir/file").unwrap();
        assert_eq!(internal, "subdir/file");
    }

    #[test]
    fn case_insensitive_extension_match() {
        assert!(parse_archive_path("/data/archive.TAR.GZ/file").is_some());
        assert!(parse_archive_path("/data/ARCHIVE.ZIP/file").is_some());
    }

    #[test]
    fn embedded_archive_extension_not_at_boundary_returns_none() {
        assert!(parse_archive_path("/home/user/my.tar.gz.bak").is_none());
    }


    #[test]
    fn is_gzip_recognizes_tar_gz() {
        assert!(is_gzip(Path::new("/path/to/archive.tar.gz")));
    }

    #[test]
    fn is_gzip_recognizes_tgz() {
        assert!(is_gzip(Path::new("archive.tgz")));
    }

    #[test]
    fn is_gzip_rejects_plain_tar() {
        assert!(!is_gzip(Path::new("archive.tar")));
    }

    #[test]
    fn is_gzip_rejects_zip() {
        assert!(!is_gzip(Path::new("archive.zip")));
    }


    #[test]
    fn is_tar_format_accepts_tar_gz() {
        assert!(is_tar_format(Path::new("archive.tar.gz")));
    }

    #[test]
    fn is_tar_format_accepts_tgz() {
        assert!(is_tar_format(Path::new("archive.tgz")));
    }

    #[test]
    fn is_tar_format_accepts_plain_tar() {
        assert!(is_tar_format(Path::new("archive.tar")));
    }

    #[test]
    fn is_tar_format_rejects_zip() {
        assert!(!is_tar_format(Path::new("archive.zip")));
    }

    #[test]
    fn is_bzip2_recognizes_every_spelling() {
        assert!(is_bzip2(Path::new("/data/backup.tar.bz2")));
        assert!(is_bzip2(Path::new("backup.tbz2")));
        assert!(is_bzip2(Path::new("backup.tbz")));
        assert!(is_bzip2(Path::new("BACKUP.TAR.BZ2")));
        assert!(!is_bzip2(Path::new("backup.tar.gz")));
        assert!(!is_bzip2(Path::new("backup.tar")));
    }

    #[test]
    fn is_tar_format_accepts_bzip2() {
        assert!(is_tar_format(Path::new("archive.tar.bz2")));
        assert!(is_tar_format(Path::new("archive.tbz2")));
        assert!(is_tar_format(Path::new("archive.tbz")));
    }

    #[test]
    fn parse_archive_path_splits_tar_bz2() {
        let (archive, internal) = parse_archive_path("/data/backup.tar.bz2/dir/file.txt").unwrap();
        assert_eq!(archive, PathBuf::from("/data/backup.tar.bz2"));
        assert_eq!(internal, "dir/file.txt");
    }

    #[test]
    fn parse_archive_path_splits_short_bzip2_spellings() {
        let (archive, internal) = parse_archive_path("/data/backup.tbz2/dir/file.txt").unwrap();
        assert_eq!(archive, PathBuf::from("/data/backup.tbz2"));
        assert_eq!(internal, "dir/file.txt");

        let (archive, internal) = parse_archive_path("/data/backup.tbz/dir/file.txt").unwrap();
        assert_eq!(archive, PathBuf::from("/data/backup.tbz"));
        assert_eq!(internal, "dir/file.txt");

        let (archive, internal) = parse_archive_path("/data/backup.tbz2").unwrap();
        assert_eq!(archive, PathBuf::from("/data/backup.tbz2"));
        assert_eq!(internal, "");
    }

    #[test]
    fn parse_archive_path_prefers_tar_bz2_over_bare_tar() {
        let (archive, _) = parse_archive_path("/data/backup.tar.bz2").unwrap();
        assert_eq!(archive, PathBuf::from("/data/backup.tar.bz2"));
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ic-archives-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn multi_stream_tar_bz2_is_read_whole() {
        use std::io::Write;
        let dir = scratch_dir("bz2multistream");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).unwrap();
        std::fs::write(payload.join("a.txt"), b"first").unwrap();
        std::fs::write(payload.join("b.txt"), b"second").unwrap();

        let plain = dir.join("payload.tar");
        {
            let mut builder = tar::Builder::new(std::fs::File::create(&plain).unwrap());
            add_tar_entries(&mut builder, &payload).unwrap();
            builder.finish().unwrap();
        }

        let raw = std::fs::read(&plain).unwrap();
        let half = raw.len() / 2;
        let mut concatenated = Vec::new();
        for chunk in [&raw[..half], &raw[half..]] {
            let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            enc.write_all(chunk).unwrap();
            concatenated.extend(enc.finish().unwrap());
        }
        let archive = dir.join("payload.tar.bz2");
        std::fs::write(&archive, &concatenated).unwrap();

        let mut decoded = Vec::new();
        let mut reader = tar_reader(&archive, std::fs::File::open(&archive).unwrap());
        std::io::Read::read_to_end(&mut reader, &mut decoded).unwrap();
        assert_eq!(
            decoded.len(),
            raw.len(),
            "only the first stream was decoded: {} of {} bytes",
            decoded.len(),
            raw.len()
        );

        assert_eq!(read_archive_file(&archive, "payload/b.txt").unwrap(), b"second");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn seed_payload(dir: &Path) -> PathBuf {
        let payload = dir.join("payload");
        std::fs::create_dir_all(payload.join("sub")).unwrap();
        std::fs::write(payload.join("top.txt"), b"top level").unwrap();
        std::fs::write(payload.join("sub").join("deep.txt"), b"nested content").unwrap();
        payload
    }

    fn assert_round_trip(tag: &str, ext: &str, magic: &[u8]) {
        let dir = scratch_dir(tag);
        let payload = seed_payload(&dir);
        let archive = dir.join(format!("payload.{ext}"));

        if ext == "zip" {
            compress_zip(&payload, &archive).unwrap();
        } else {
            compress_tar(&payload, &archive).unwrap();
        }
        assert!(archive.is_file(), "{ext}: archive was not created");

        let head = std::fs::read(&archive).unwrap();
        assert_eq!(&head[..magic.len()], magic, "{ext}: wrong container magic");

        let (dirs, files) = list_archive_directory(&archive, "payload").unwrap();
        assert!(dirs.iter().any(|d| d == "sub"), "{ext}: missing dir, got {dirs:?}");
        assert!(
            files.iter().any(|(n, _)| n == "top.txt"),
            "{ext}: missing file, got {files:?}"
        );

        let content = read_archive_file(&archive, "payload/sub/deep.txt").unwrap();
        assert_eq!(content, b"nested content", "{ext}: wrong content read back");

        let dest = dir.join("out");
        if ext == "zip" {
            extract_zip(&archive, &dest).unwrap();
        } else {
            extract_tar(&archive, &dest).unwrap();
        }
        assert_eq!(
            std::fs::read(dest.join("payload").join("sub").join("deep.txt")).unwrap(),
            b"nested content",
            "{ext}: extraction lost the nested file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_round_trips() {
        assert_round_trip("zip", "zip", b"PK");
    }

    #[test]
    fn plain_tar_round_trips() {
        let dir = scratch_dir("plaintar");
        let payload = seed_payload(&dir);
        let archive = dir.join("payload.tar");
        compress_tar(&payload, &archive).unwrap();
        let dest = dir.join("out");
        extract_tar(&archive, &dest).unwrap();
        assert_eq!(
            std::fs::read(dest.join("payload").join("top.txt")).unwrap(),
            b"top level"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_gz_round_trips() {
        assert_round_trip("targz", "tar.gz", &[0x1f, 0x8b]);
    }

    #[test]
    fn tar_bz2_round_trips() {
        assert_round_trip("tarbz2", "tar.bz2", b"BZh");
    }
}

fn is_gzip(path: &Path) -> bool {
    let p_lower = path.to_string_lossy().to_lowercase();
    p_lower.ends_with(".tar.gz") || p_lower.ends_with(".tgz")
}

fn is_bzip2(path: &Path) -> bool {
    let p_lower = path.to_string_lossy().to_lowercase();
    p_lower.ends_with(".tar.bz2") || p_lower.ends_with(".tbz2") || p_lower.ends_with(".tbz")
}

pub fn is_tar_format(path: &Path) -> bool {
    let p_lower = path.to_string_lossy().to_lowercase();
    p_lower.ends_with(".tar") || is_gzip(path) || is_bzip2(path)
}

fn tar_reader(path: &Path, file: std::fs::File) -> Box<dyn std::io::Read> {
    if is_gzip(path) {
        Box::new(flate2::read::GzDecoder::new(file))
    } else if is_bzip2(path) {
        Box::new(bzip2::read::MultiBzDecoder::new(file))
    } else {
        Box::new(file)
    }
}

pub fn list_archive_directory(
    archive_path: &Path,
    internal_dir: &str,
) -> Result<(Vec<String>, Vec<(String, u64)>), String> {
    if is_tar_format(archive_path) {
        list_tar_directory(archive_path, internal_dir)
    } else {
        list_zip_directory(archive_path, internal_dir)
    }
}

fn list_zip_directory(
    archive_path: &Path,
    internal_dir: &str,
) -> Result<(Vec<String>, Vec<(String, u64)>), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let internal_dir = internal_dir.replace('\\', "/");
    let internal_dir = internal_dir.trim_start_matches('/').trim_end_matches('/');

    let mut dirs = HashSet::new();
    let mut files = Vec::new();
    let mut seen_files = HashSet::new();

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let name_stripped = name.trim_start_matches('/');

        if internal_dir.is_empty() {
            if name_stripped.is_empty() {
                continue;
            }
            let parts: Vec<&str> = name_stripped.split('/').collect();
            if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                dirs.insert(parts[0].to_string());
            } else {
                if entry.is_dir() || name_stripped.ends_with('/') {
                    dirs.insert(parts[0].to_string());
                } else {
                    let filename = parts[0].to_string();
                    if !seen_files.contains(&filename) {
                        seen_files.insert(filename.clone());
                        files.push((filename, entry.size()));
                    }
                }
            }
        } else {
            let prefix = format!("{}/", internal_dir);
            if name_stripped.starts_with(&prefix) {
                let subpath = &name_stripped[prefix.len()..];
                if subpath.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = subpath.split('/').collect();
                if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                    dirs.insert(parts[0].to_string());
                } else {
                    if entry.is_dir() || subpath.ends_with('/') {
                        dirs.insert(parts[0].to_string());
                    } else {
                        let filename = parts[0].to_string();
                        if !seen_files.contains(&filename) {
                            seen_files.insert(filename.clone());
                            files.push((filename, entry.size()));
                        }
                    }
                }
            }
        }
    }

    files.retain(|(name, _)| !dirs.contains(name));

    let mut dirs_vec: Vec<String> = dirs.into_iter().collect();
    dirs_vec.sort();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((dirs_vec, files))
}

fn list_tar_directory(
    archive_path: &Path,
    internal_dir: &str,
) -> Result<(Vec<String>, Vec<(String, u64)>), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;

    let internal_dir = internal_dir.replace('\\', "/");
    let internal_dir = internal_dir.trim_start_matches('/').trim_end_matches('/');

    let mut dirs = HashSet::new();
    let mut files = Vec::new();
    let mut seen_files = HashSet::new();

    let mut process_entry = |name_str: &str, is_dir: bool, size: u64| -> Result<(), String> {
        let name = name_str.replace('\\', "/");
        let name_stripped = name.trim_start_matches('/');

        if internal_dir.is_empty() {
            if name_stripped.is_empty() {
                return Ok(());
            }
            let parts: Vec<&str> = name_stripped.split('/').collect();
            if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                dirs.insert(parts[0].to_string());
            } else {
                if is_dir || name_stripped.ends_with('/') {
                    dirs.insert(parts[0].to_string());
                } else {
                    let filename = parts[0].to_string();
                    if !seen_files.contains(&filename) {
                        seen_files.insert(filename.clone());
                        files.push((filename, size));
                    }
                }
            }
        } else {
            let prefix = format!("{}/", internal_dir);
            if name_stripped.starts_with(&prefix) {
                let subpath = &name_stripped[prefix.len()..];
                if subpath.is_empty() {
                    return Ok(());
                }
                let parts: Vec<&str> = subpath.split('/').collect();
                if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                    dirs.insert(parts[0].to_string());
                } else {
                    if is_dir || subpath.ends_with('/') {
                        dirs.insert(parts[0].to_string());
                    } else {
                        let filename = parts[0].to_string();
                        if !seen_files.contains(&filename) {
                            seen_files.insert(filename.clone());
                            files.push((filename, size));
                        }
                    }
                }
            }
        }
        Ok(())
    };

    let mut tar = tar::Archive::new(tar_reader(archive_path, file));
    for entry_res in tar.entries().map_err(|e| e.to_string())? {
        let entry = entry_res.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let path_str = path.to_string_lossy();
        let is_dir = entry.header().entry_type().is_dir();
        process_entry(&path_str, is_dir, entry.size())?;
    }

    files.retain(|(name, _)| !dirs.contains(name));

    let mut dirs_vec: Vec<String> = dirs.into_iter().collect();
    dirs_vec.sort();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((dirs_vec, files))
}

pub fn read_archive_file(archive_path: &Path, internal_file: &str) -> Result<Vec<u8>, String> {
    if is_tar_format(archive_path) {
        read_tar_file(archive_path, internal_file)
    } else {
        read_zip_file(archive_path, internal_file)
    }
}

fn read_zip_file(archive_path: &Path, internal_file: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let internal_file_norm = internal_file.replace('\\', "/");
    let internal_file_norm = internal_file_norm.trim_start_matches('/');

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let entry_name = entry.name().replace('\\', "/");
        if entry_name.trim_start_matches('/') == internal_file_norm {
            if entry.is_dir() {
                return Err("Is a directory".to_string());
            }
            let mut content = Vec::with_capacity(entry.size() as usize);
            std::io::copy(&mut entry, &mut content).map_err(|e| e.to_string())?;
            return Ok(content);
        }
    }
    Err(format!("File not found in archive: {}", internal_file))
}

fn read_tar_file(archive_path: &Path, internal_file: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;

    let internal_file_norm = internal_file.replace('\\', "/");
    let internal_file_norm = internal_file_norm.trim_start_matches('/');

    let mut tar = tar::Archive::new(tar_reader(archive_path, file));
    for entry_res in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry_res.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let entry_name = path.to_string_lossy().replace('\\', "/");
        if entry_name.trim_start_matches('/') == internal_file_norm {
            if entry.header().entry_type().is_dir() {
                return Err("Is a directory".to_string());
            }
            let mut content = Vec::with_capacity(entry.size() as usize);
            std::io::copy(&mut entry, &mut content).map_err(|e| e.to_string())?;
            return Ok(content);
        }
    }
    Err(format!("File not found in archive: {}", internal_file))
}

pub fn extract_zip(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match entry.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if entry.is_dir() || entry.name().ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode)).ok();
            }
        }
    }
    Ok(())
}

pub fn extract_tar(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

    let mut archive = tar::Archive::new(tar_reader(archive_path, file));
    archive.unpack(dest_dir).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn compress_zip(entry_path: &Path, archive_path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(archive_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    if entry_path.is_dir() {
        let walk = walkdir::WalkDir::new(entry_path);
        for entry in walk.into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path
                .strip_prefix(entry_path.parent().unwrap_or(entry_path))
                .map_err(|e| e.to_string())?;

            let name_str = name
                .to_str()
                .ok_or("Invalid path encoding")?
                .replace('\\', "/");
            if name_str.is_empty() {
                continue;
            }

            if path.is_dir() {
                zip.add_directory(format!("{}/", name_str), options)
                    .map_err(|e| e.to_string())?;
            } else {
                zip.start_file(name_str, options)
                    .map_err(|e| e.to_string())?;
                let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
            }
        }
    } else {
        let name_str = entry_path
            .file_name()
            .ok_or("No filename found")?
            .to_str()
            .ok_or("Invalid path encoding")?;
        zip.start_file(name_str, options)
            .map_err(|e| e.to_string())?;
        let mut f = std::fs::File::open(entry_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn compress_tar(entry_path: &Path, archive_path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(archive_path).map_err(|e| e.to_string())?;

    if is_gzip(archive_path) {
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        add_tar_entries(&mut builder, entry_path)?;
        builder
            .into_inner()
            .map_err(|e| e.to_string())?
            .finish()
            .map_err(|e| e.to_string())?;
    } else if is_bzip2(archive_path) {
        let enc = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        add_tar_entries(&mut builder, entry_path)?;
        builder
            .into_inner()
            .map_err(|e| e.to_string())?
            .finish()
            .map_err(|e| e.to_string())?;
    } else {
        let mut builder = tar::Builder::new(file);
        add_tar_entries(&mut builder, entry_path)?;
        builder.finish().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn add_tar_entries<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    entry_path: &Path,
) -> Result<(), String> {
    if entry_path.is_dir() {
        let parent = entry_path.parent().unwrap_or(entry_path);
        let walk = walkdir::WalkDir::new(entry_path);
        for entry in walk.into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.strip_prefix(parent).map_err(|e| e.to_string())?;
            if path.is_dir() {
                builder.append_dir(name, path).map_err(|e| e.to_string())?;
            } else {
                builder
                    .append_path_with_name(path, name)
                    .map_err(|e| e.to_string())?;
            }
        }
    } else {
        let name = entry_path.file_name().ok_or("No filename found")?;
        builder
            .append_path_with_name(entry_path, name)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
