use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_path_string(parts: &[String]) -> String {
    if parts.is_empty() {
        return "/".to_string();
    }
    let mut path = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            if p.ends_with(':') {
                path = format!("{}/", p);
            } else {
                path = format!("/{}", p);
            }
        } else {
            if !path.ends_with('/') && !path.ends_with('\\') {
                path.push('/');
            }
            path.push_str(p);
        }
    }
    path
}

pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

pub fn update_status_bar_text(
    status_label: &gtk::Label,
    selection_model: &gtk::MultiSelection,
    _list_store: &gtk::gio::ListStore,
    cached_entries: &Rc<RefCell<Vec<(String, bool, u64, String, Option<u32>)>>>,
) {
    let bitset = selection_model.selection();
    let status_text = if bitset.is_empty() {
        let entries = cached_entries.borrow();
        let folders = entries
            .iter()
            .filter(|(_, is_dir, _, _, _)| *is_dir)
            .count();
        let files = entries
            .iter()
            .filter(|(_, is_dir, _, _, _)| !*is_dir)
            .count();
        if folders == 0 && files == 0 {
            crate::i18n::tr("fm.status.empty_folder").to_string()
        } else {
            let folders_str = if folders == 1 {
                crate::i18n::tr("fm.status.one_folder").to_string()
            } else {
                crate::i18n::trf("fm.status.many_folders", &[("count", &*(folders).to_string())]).to_string()
            };
            let files_str = if files == 1 {
                crate::i18n::tr("fm.status.one_file").to_string()
            } else {
                crate::i18n::trf("fm.status.many_files", &[("count", &*(files).to_string())]).to_string()
            };
            format!("{}, {}", folders_str, files_str)
        }
    } else if bitset.size() == 1 {
        let pos = bitset.nth(0);
        if let Some(obj) = selection_model.item(pos) {
            if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                let mut name = entry.name();
                if name.chars().count() > 50 {
                    let mut name_trunc: String = name.chars().take(47).collect();
                    name_trunc.push_str("...");
                    name = name_trunc;
                }
                let date = entry.date();
                let date_part = if date.is_empty() {
                    "".to_string()
                } else {
                    format!(" | {}", date)
                };
                if entry.is_dir() {
                    crate::i18n::trf("fm.status.selected_folder", &[("name", &*(name).to_string()), ("date", &*(date_part).to_string())])
                        .to_string()
                } else {
                    crate::i18n::trf("fm.status.selected_file", &[("name", &*(name).to_string()), ("size", &*(format_size(entry.size())).to_string()), ("date", &*(date_part).to_string())])
                    .to_string()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        let mut sel_folders = 0;
        let mut sel_files = 0;
        for i in 0..bitset.size() {
            let pos = bitset.nth(i as u32);
            if let Some(obj) = selection_model.item(pos) {
                if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                    if entry.is_dir() {
                        sel_folders += 1;
                    } else {
                        sel_files += 1;
                    }
                }
            }
        }
        let folders_str = if sel_folders == 1 {
            crate::i18n::tr("fm.status.one_folder").to_string()
        } else {
            crate::i18n::trf("fm.status.many_folders", &[("count", &*(sel_folders).to_string())]).to_string()
        };
        let files_str = if sel_files == 1 {
            crate::i18n::tr("fm.status.one_file").to_string()
        } else {
            crate::i18n::trf("fm.status.many_files", &[("count", &*(sel_files).to_string())]).to_string()
        };
        crate::i18n::trf("fm.status.selected_multiple", &[("folders", &*(folders_str).to_string()), ("files", &*(files_str).to_string())])
        .to_string()
    };
    status_label.set_text(&status_text);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIcon {
    Resource(String),
    IconName(String),
    GeneratedSvg(String),
}

pub fn get_file_icon(name: &str, is_dir: bool, size: u32, permissions: Option<u32>) -> FileIcon {
    let resource_size = size;

    if is_dir {
        return FileIcon::Resource("/com/fm-ui/gtk/folder.svg".to_string());
    }

    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let lower_name = name.to_lowercase();
    let is_tar_compound = lower_name.ends_with(".tar.gz") || lower_name.ends_with(".tar.bz2");

    let mapped_type = if is_tar_compound {
        Some((crate::icon_generator::FileType::Archive, "TAR".to_string()))
    } else {
        match ext.as_str() {
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "tbz2" | "tbz" => {
                let text = if ext == "7z" {
                    "7Z".to_string()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::Archive, text))
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => {
                let text = if ext.len() > 4 {
                    ext[..4].to_uppercase()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::Document, text))
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => {
                let text = if ext == "jpeg" {
                    "JPG".to_string()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::Photo, text))
            }
            "nef" | "cr2" | "cr3" | "arw" | "dng" | "raf" | "orf" | "rw2" | "pef" => {
                Some((crate::icon_generator::FileType::Photo, "RAW".to_string()))
            }
            "rs" | "js" | "ts" | "html" | "css" | "go" | "py" | "cpp" | "c" | "h" | "cs"
            | "java" => {
                let text = if ext.len() > 4 {
                    ext[..4].to_uppercase()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::Developer, text))
            }
            "mp3" | "wav" | "ogg" | "flac" | "mp4" | "mkv" | "avi" | "mov" => {
                let text = if ext.len() > 4 {
                    ext[..4].to_uppercase()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::Media, text))
            }
            "txt" | "ini" | "conf" | "toml" | "yaml" | "yml" | "json" => {
                let text = if ext.len() > 4 {
                    ext[..4].to_uppercase()
                } else {
                    ext.to_uppercase()
                };
                Some((crate::icon_generator::FileType::ConfigText, text))
            }
            "apk" => Some((
                crate::icon_generator::FileType::Executable,
                "APK".to_string(),
            )),
            "exe" => Some((
                crate::icon_generator::FileType::Executable,
                "EXE".to_string(),
            )),
            "dll" => Some((
                crate::icon_generator::FileType::Developer,
                "DLL".to_string(),
            )),
            _ => {
                if let Some(mode) = permissions {
                    if (mode & 0o111) != 0 {
                        Some((
                            crate::icon_generator::FileType::Executable,
                            "BIN".to_string(),
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    };

    if let Some((file_type, text)) = mapped_type {
        let svg = crate::icon_generator::generate_svg_icon(&text, file_type, size);
        FileIcon::GeneratedSvg(svg)
    } else {
        if resource_size == 30 {
            let icon = match ext.as_str() {
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "nef" | "cr2" | "cr3" | "arw"
                | "dng" | "raf" | "orf" | "rw2" | "pef" => "image-x-generic",
                "mp4" | "mkv" | "avi" | "webm" | "mov" => "video-x-generic",
                "mp3" | "wav" | "ogg" | "flac" => "audio-x-generic",
                "rs" | "js" | "ts" | "html" | "css" | "json" | "toml" | "yml" | "txt" | "ini" => {
                    "text-x-script"
                }
                "pdf" => "application-pdf",
                "exe" | "sh" | "bash" | "bin" => "application-x-executable",
                _ => "text-x-generic",
            };
            if icon == "text-x-generic" || icon == "text-x-script" {
                FileIcon::Resource("/com/fm-ui/gtk/file.svg".to_string())
            } else {
                FileIcon::IconName(icon.to_string())
            }
        } else {
            FileIcon::Resource("/com/fm-ui/gtk/file.svg".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon_generator::{generate_svg_icon, FileType};

    const FOLDER_RES: &str = "/com/fm-ui/gtk/folder.svg";
    const FILE_RES: &str = "/com/fm-ui/gtk/file.svg";
    const LIST_SIZE: u32 = 30;

    fn parts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_parts_build_the_root_path() {
        assert_eq!(build_path_string(&[]), "/");
    }

    #[test]
    fn unix_parts_are_joined_under_a_leading_slash() {
        assert_eq!(build_path_string(&parts(&["tmp"])), "/tmp");
        assert_eq!(
            build_path_string(&parts(&["home", "ice", "docs"])),
            "/home/ice/docs"
        );
    }

    #[test]
    fn windows_drive_letter_becomes_the_root_without_a_doubled_slash() {
        assert_eq!(build_path_string(&parts(&["C:"])), "C:/");
        assert_eq!(
            build_path_string(&parts(&["C:", "Users", "ice"])),
            "C:/Users/ice"
        );
    }

    #[test]
    fn a_part_ending_in_a_backslash_is_not_given_another_separator() {
        assert_eq!(
            build_path_string(&parts(&["C:", "dir\\", "file.txt"])),
            "C:/dir\\file.txt"
        );
    }

    #[test]
    fn unicode_and_spaces_pass_through_path_building() {
        assert_eq!(
            build_path_string(&parts(&["дом", "мои файлы", "🎧.mp3"])),
            "/дом/мои файлы/🎧.mp3"
        );
    }

    #[test]
    fn sizes_below_one_kilobyte_are_printed_as_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn unit_switches_happen_exactly_at_the_powers_of_1024() {
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024 - 1), "1024.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024 - 1), "1024.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn fractional_sizes_keep_two_decimals() {
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024 + 512 * 1024 * 1024), "3.50 GB");
    }

    #[test]
    fn the_largest_size_stays_in_gigabytes() {
        let text = format_size(u64::MAX);
        assert!(text.ends_with(" GB"), "unexpected unit in {text}");
        assert!(text.starts_with("17179869184"), "unexpected value in {text}");
    }

    fn assert_classified(name: &str, label: &str, file_type: FileType) {
        assert_eq!(
            get_file_icon(name, false, LIST_SIZE, None),
            FileIcon::GeneratedSvg(generate_svg_icon(label, file_type, LIST_SIZE)),
            "wrong icon for {name}"
        );
    }

    #[test]
    fn directories_always_get_the_folder_resource() {
        assert_eq!(
            get_file_icon("photos", true, LIST_SIZE, None),
            FileIcon::Resource(FOLDER_RES.to_string())
        );
        assert_eq!(
            get_file_icon("archive.zip", true, 80, Some(0o755)),
            FileIcon::Resource(FOLDER_RES.to_string())
        );
        assert_eq!(
            get_file_icon("..", true, LIST_SIZE, None),
            FileIcon::Resource(FOLDER_RES.to_string())
        );
    }

    #[test]
    fn compound_tar_names_are_labelled_tar() {
        assert_classified("backup.tar.gz", "TAR", FileType::Archive);
        assert_classified("backup.tar.bz2", "TAR", FileType::Archive);
        assert_classified("BACKUP.TAR.GZ", "TAR", FileType::Archive);
    }

    #[test]
    fn short_tar_aliases_keep_their_own_label() {
        assert_classified("backup.tgz", "TGZ", FileType::Archive);
        assert_classified("backup.tbz2", "TBZ2", FileType::Archive);
        assert_classified("backup.tbz", "TBZ", FileType::Archive);
        assert_classified("backup.bz2", "BZ2", FileType::Archive);
        assert_classified("backup.tar", "TAR", FileType::Archive);
    }

    #[test]
    fn seven_zip_gets_a_leading_digit_label() {
        assert_classified("dump.7z", "7Z", FileType::Archive);
        assert_classified("dump.rar", "RAR", FileType::Archive);
        assert_classified("dump.zip", "ZIP", FileType::Archive);
    }

    #[test]
    fn jpeg_is_labelled_jpg_like_its_short_spelling() {
        assert_classified("holiday.jpeg", "JPG", FileType::Photo);
        assert_classified("holiday.jpg", "JPG", FileType::Photo);
        assert_classified("HOLIDAY.JPEG", "JPG", FileType::Photo);
    }

    #[test]
    fn raw_camera_extensions_collapse_into_one_raw_label() {
        assert_classified("IMG_0001.NEF", "RAW", FileType::Photo);
        assert_classified("img.cr3", "RAW", FileType::Photo);
        assert_classified("img.rw2", "RAW", FileType::Photo);
    }

    #[test]
    fn documents_sources_media_and_config_use_their_own_palettes() {
        assert_classified("report.pdf", "PDF", FileType::Document);
        assert_classified("sheet.xlsx", "XLSX", FileType::Document);
        assert_classified("main.rs", "RS", FileType::Developer);
        assert_classified("index.html", "HTML", FileType::Developer);
        assert_classified("lib.dll", "DLL", FileType::Developer);
        assert_classified("song.flac", "FLAC", FileType::Media);
        assert_classified("movie.mkv", "MKV", FileType::Media);
        assert_classified("Cargo.toml", "TOML", FileType::ConfigText);
        assert_classified("ci.yaml", "YAML", FileType::ConfigText);
    }

    #[test]
    fn installer_extensions_are_executables_without_any_permission_bits() {
        assert_classified("app.apk", "APK", FileType::Executable);
        assert_classified("setup.exe", "EXE", FileType::Executable);
    }

    #[test]
    fn a_known_extension_wins_over_the_executable_bit() {
        assert_eq!(
            get_file_icon("notes.txt", false, LIST_SIZE, Some(0o755)),
            FileIcon::GeneratedSvg(generate_svg_icon("TXT", FileType::ConfigText, LIST_SIZE))
        );
    }

    #[test]
    fn any_executable_bit_turns_an_unknown_file_into_a_binary() {
        let bin = FileIcon::GeneratedSvg(generate_svg_icon("BIN", FileType::Executable, LIST_SIZE));
        assert_eq!(get_file_icon("tool.bundle", false, LIST_SIZE, Some(0o755)), bin);
        assert_eq!(get_file_icon("tool.bundle", false, LIST_SIZE, Some(0o001)), bin);
        assert_eq!(
            get_file_icon("tool.bundle", false, LIST_SIZE, Some(0o644)),
            FileIcon::Resource(FILE_RES.to_string())
        );
    }

    #[test]
    fn unknown_and_extensionless_names_fall_back_to_the_file_resource() {
        assert_eq!(
            get_file_icon("notes.qqq", false, LIST_SIZE, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
        assert_eq!(
            get_file_icon("README", false, LIST_SIZE, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
        assert_eq!(
            get_file_icon("", false, LIST_SIZE, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
    }

    #[test]
    fn a_leading_dot_is_not_an_extension() {
        assert_eq!(
            get_file_icon(".bashrc", false, LIST_SIZE, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
    }

    #[test]
    fn themed_icon_names_are_used_only_at_list_size() {
        assert_eq!(
            get_file_icon("clip.webm", false, LIST_SIZE, None),
            FileIcon::IconName("video-x-generic".to_string())
        );
        assert_eq!(
            get_file_icon("clip.webm", false, 80, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
        assert_eq!(
            get_file_icon("clip.webm", false, 25, None),
            FileIcon::Resource(FILE_RES.to_string())
        );
    }

    #[test]
    fn shell_scripts_without_the_executable_bit_use_the_theme_executable_icon() {
        assert_eq!(
            get_file_icon("run.sh", false, LIST_SIZE, None),
            FileIcon::IconName("application-x-executable".to_string())
        );
        assert_eq!(
            get_file_icon("run.sh", false, LIST_SIZE, Some(0o755)),
            FileIcon::GeneratedSvg(generate_svg_icon("BIN", FileType::Executable, LIST_SIZE))
        );
    }
}
