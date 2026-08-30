#![allow(deprecated)] // adw::MessageDialog — modernization tracked separately
use adw::prelude::*;
use relm4::ComponentSender;

use crate::fm_view::{FmPanelModel, FmPanelOutput, Shared};

fn unique_copy_name(name: &str, is_dir: bool, existing: &[String]) -> String {
    let (stem, ext) = match (is_dir, name.rfind('.')) {
        (false, Some(i)) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            format!("{stem} copy{ext}")
        } else {
            format!("{stem} copy {n}{ext}")
        };
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_context_menu(
    name: &str,
    _size: u64,
    is_dir: bool,
    parent_path: String,
    shared: &Shared,
    root_path: String,
    sender: &ComponentSender<FmPanelModel>,
    btn_delete: &gtk::Button,
    _btn_refresh: &gtk::Button,
    btn_chmod: &gtk::Button,
) -> gtk::Popover {
    let popover = gtk::Popover::builder()
        .position(gtk::PositionType::Bottom)
        .has_arrow(true)
        .build();
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    let name_clone = name.to_string();
    let safe_parent = if parent_path.ends_with('/') {
        parent_path.clone()
    } else {
        format!("{}/", parent_path)
    };

    let create_btn_with_label = |label: &str, icon: &str, css: &str| -> gtk::Button {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        b.append(&gtk::Image::from_icon_name(icon));
        b.append(&gtk::Label::new(Some(label)));
        let mut classes = vec!["flat"];
        if !css.is_empty() {
            classes.push(css);
        }
        gtk::Button::builder()
            .child(&b)
            .css_classes(classes)
            .build()
    };

    let btn_copy = create_btn_with_label(
        &crate::i18n::tr("fm.context.copy_path"),
        "edit-copy-symbolic",
        "",
    );
    let pop_copy = popover.clone();
    let name_c = name_clone.clone();
    let root_path_c = root_path.clone();
    let safe_parent_c = safe_parent.clone();
    btn_copy.connect_clicked(move |_| {
        pop_copy.popdown();
        let full_item_path = format!("{}{}", safe_parent_c, name_c);
        #[cfg(feature = "support-archives")]
        let copy_text = if let Some((_archive_path, internal_path)) =
            client_archives::parse_archive_path(&full_item_path)
        {
            if internal_path.is_empty() {
                relative_to_root(&full_item_path, &root_path_c)
            } else if internal_path.starts_with('/') {
                internal_path
            } else {
                format!("/{}", internal_path)
            }
        } else {
            relative_to_root(&full_item_path, &root_path_c)
        };
        #[cfg(not(feature = "support-archives"))]
        let copy_text = relative_to_root(&full_item_path, &root_path_c);

        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&copy_text);
        }
    });
    vbox.append(&btn_copy);
    vbox.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build(),
    );

    if !is_dir {
        let btn_dl = create_btn_with_label(
            &crate::i18n::tr("fm.context.download"),
            "folder-download-symbolic",
            "",
        );
        let f_path = format!("{}{}", safe_parent, name_clone);
        let pop_dl = popover.clone();
        let sender_dl = sender.clone();
        btn_dl.connect_clicked(move |_| {
            let _ = sender_dl.output(FmPanelOutput::Download {
                paths: vec![f_path.clone()],
            });
            pop_dl.popdown();
        });
        vbox.append(&btn_dl);
    }

    let btn_ren = create_btn_with_label(
        &crate::i18n::tr("fm.context.rename"),
        "document-edit-symbolic",
        "",
    );
    let pop_ren = popover.clone();
    let sender_ren = sender.clone();
    let parent_ren = parent_path.clone();
    let name_ren = name_clone.clone();
    btn_ren.connect_clicked(move |_| {
        pop_ren.popdown();
        if let Some(window) = pop_ren
            .parent()
            .and_then(|p| p.root())
            .and_downcast::<gtk::Window>()
        {
            crate::dialogs::rename_dialog_for(&window, &parent_ren, &name_ren, sender_ren.clone());
        }
    });
    vbox.append(&btn_ren);

    let btn_dup = create_btn_with_label(
        &crate::i18n::tr("fm.context.duplicate"),
        "edit-copy-symbolic",
        "",
    );
    let pop_dup = popover.clone();
    let sender_dup = sender.clone();
    let dup_parent = safe_parent.clone();
    let dup_name = name_clone.clone();
    let existing_names: Vec<String> = shared
        .cached_entries
        .borrow()
        .iter()
        .map(|(n, _, _, _, _)| n.clone())
        .collect();
    btn_dup.connect_clicked(move |_| {
        pop_dup.popdown();
        let dest_name = unique_copy_name(&dup_name, is_dir, &existing_names);
        let _ = sender_dup.output(FmPanelOutput::Duplicate {
            src: format!("{}{}", dup_parent, dup_name),
            dst: format!("{}{}", dup_parent, dest_name),
        });
    });
    vbox.append(&btn_dup);

    let btn_del = create_btn_with_label(
        &crate::i18n::tr("fm.context.delete"),
        "user-trash-symbolic",
        "destructive-action",
    );
    let pop_del = popover.clone();
    let btn_delete_clone = btn_delete.clone();
    btn_del.connect_clicked(move |_| {
        pop_del.popdown();
        btn_delete_clone.activate();
    });
    vbox.append(&btn_del);

    let btn_chm = create_btn_with_label("Permissions / Chmod", "dialog-password-symbolic", "");
    let pop_chm = popover.clone();
    let btn_chmod_clone = btn_chmod.clone();
    btn_chm.connect_clicked(move |_| {
        pop_chm.popdown();
        btn_chmod_clone.activate();
    });
    vbox.append(&btn_chm);

    #[cfg(feature = "support-archives")]
    {
        let lower = name_clone.to_lowercase();
        let is_archive = !is_dir
            && (lower.ends_with(".zip")
                || lower.ends_with(".tar")
                || lower.ends_with(".tar.gz")
                || lower.ends_with(".tgz")
                || lower.ends_with(".tar.bz2")
                || lower.ends_with(".tbz2")
                || lower.ends_with(".tbz"));
        if is_archive {
            let btn_ext = create_btn_with_label(
                &crate::i18n::tr("fm.context.extract"),
                "archive-insert-symbolic",
                "",
            );
            let f_path = format!("{}{}", safe_parent, name_clone);
            let pop_ext = popover.clone();
            let sender_ext = sender.clone();
            btn_ext.connect_clicked(move |_| {
                pop_ext.popdown();
                let _ = sender_ext.output(FmPanelOutput::Extract {
                    archive_path: f_path.clone(),
                });
            });
            vbox.append(&btn_ext);
        }

        let f_path = format!("{}{}", safe_parent, name_clone);
        let btn_zip = create_btn_with_label(
            &crate::i18n::tr("fm.context.compress_zip"),
            "package-x-generic-symbolic",
            "",
        );
        let pop_zip = popover.clone();
        let sender_zip = sender.clone();
        let src_zip = f_path.clone();
        btn_zip.connect_clicked(move |_| {
            pop_zip.popdown();
            let _ = sender_zip.output(FmPanelOutput::Compress {
                src: src_zip.clone(),
                dest: format!("{}.zip", src_zip),
            });
        });
        vbox.append(&btn_zip);

        let btn_tar = create_btn_with_label(
            &crate::i18n::tr("fm.context.compress_tar"),
            "package-x-generic-symbolic",
            "",
        );
        let pop_tar = popover.clone();
        let sender_tar = sender.clone();
        let src_tar = f_path.clone();
        btn_tar.connect_clicked(move |_| {
            pop_tar.popdown();
            let _ = sender_tar.output(FmPanelOutput::Compress {
                src: src_tar.clone(),
                dest: format!("{}.tar.gz", src_tar),
            });
        });
        vbox.append(&btn_tar);

        let btn_tbz = create_btn_with_label(
            &crate::i18n::tr("fm.context.compress_tar_bz2"),
            "package-x-generic-symbolic",
            "",
        );
        let pop_tbz = popover.clone();
        let sender_tbz = sender.clone();
        let src_tbz = f_path;
        btn_tbz.connect_clicked(move |_| {
            pop_tbz.popdown();
            let _ = sender_tbz.output(FmPanelOutput::Compress {
                src: src_tbz.clone(),
                dest: format!("{}.tar.bz2", src_tbz),
            });
        });
        vbox.append(&btn_tbz);
    }

    popover.set_child(Some(&vbox));
    popover
}

fn relative_to_root(full_item_path: &str, root_path: &str) -> String {
    if !root_path.is_empty() && root_path != "/" && full_item_path.starts_with(root_path) {
        let mut rel = full_item_path[root_path.len()..].to_string();
        if !rel.starts_with('/') {
            rel = format!("/{}", rel);
        }
        rel
    } else {
        full_item_path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_first_copy_is_suffixed_before_the_extension() {
        assert_eq!(unique_copy_name("notes.txt", false, &[]), "notes copy.txt");
    }

    #[test]
    fn copies_are_numbered_from_two_once_the_plain_name_is_taken() {
        assert_eq!(
            unique_copy_name("notes.txt", false, &names(&["notes copy.txt"])),
            "notes copy 2.txt"
        );
        assert_eq!(
            unique_copy_name(
                "notes.txt",
                false,
                &names(&["notes copy.txt", "notes copy 2.txt"])
            ),
            "notes copy 3.txt"
        );
    }

    #[test]
    fn a_gap_in_the_numbering_is_reused() {
        assert_eq!(
            unique_copy_name(
                "notes.txt",
                false,
                &names(&["notes copy.txt", "notes copy 3.txt"])
            ),
            "notes copy 2.txt"
        );
    }

    #[test]
    fn unrelated_neighbours_do_not_shift_the_numbering() {
        assert_eq!(
            unique_copy_name("notes.txt", false, &names(&["other copy.txt", "notes.txt"])),
            "notes copy.txt"
        );
    }

    #[test]
    fn directories_keep_a_dotted_name_intact() {
        assert_eq!(unique_copy_name("photos", true, &[]), "photos copy");
        assert_eq!(unique_copy_name("my.backup", true, &[]), "my.backup copy");
    }

    #[test]
    fn a_dotfile_is_not_split_into_stem_and_extension() {
        assert_eq!(unique_copy_name(".bashrc", false, &[]), ".bashrc copy");
    }

    #[test]
    fn only_the_last_extension_is_preserved() {
        assert_eq!(
            unique_copy_name("archive.tar.gz", false, &[]),
            "archive.tar copy.gz"
        );
    }

    #[test]
    fn a_file_without_an_extension_gets_the_suffix_appended() {
        assert_eq!(unique_copy_name("README", false, &[]), "README copy");
    }

    #[test]
    fn unicode_names_keep_their_extension_split() {
        assert_eq!(unique_copy_name("отчёт.pdf", false, &[]), "отчёт copy.pdf");
    }

    #[test]
    fn an_empty_root_leaves_the_path_untouched() {
        assert_eq!(relative_to_root("/home/ice/a.txt", ""), "/home/ice/a.txt");
    }

    #[test]
    fn the_filesystem_root_leaves_the_path_untouched() {
        assert_eq!(relative_to_root("/home/ice/a.txt", "/"), "/home/ice/a.txt");
    }

    #[test]
    fn a_matching_root_is_stripped_and_the_rest_stays_absolute() {
        assert_eq!(
            relative_to_root("/home/ice/docs/a.txt", "/home/ice"),
            "/docs/a.txt"
        );
        assert_eq!(
            relative_to_root("/home/ice/docs/a.txt", "/home/ice/"),
            "/docs/a.txt"
        );
    }

    #[test]
    fn the_root_itself_maps_to_a_single_slash() {
        assert_eq!(relative_to_root("/home/ice", "/home/ice"), "/");
    }

    #[test]
    fn a_path_outside_the_root_is_returned_as_is() {
        assert_eq!(relative_to_root("/var/log/x", "/home/ice"), "/var/log/x");
    }
}
