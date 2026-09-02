use adw::prelude::*;
use gtk::glib;
use gtk::{Box, Button, Label, Orientation, ScrolledWindow};

pub fn show_help_dialog(parent_window: &impl IsA<gtk::Window>, config: &client_config::AppConfig) {
    let help_window = adw::Window::builder()
        .title(&*crate::i18n::tr("help.title"))
        .transient_for(parent_window)
        .modal(true)
        .default_width(650)
        .default_height(550)
        .resizable(true)
        .build();

    let root_vbox = Box::builder().orientation(Orientation::Vertical).build();

    let header_bar = adw::HeaderBar::new();
    root_vbox.append(&header_bar);

    let hotkeys = crate::hotkey::get_hotkeys(config);
    let get_keys = |id: &str| -> String {
        hotkeys.iter()
            .find(|h| h.id == id)
            .map(|h| h.keys.clone())
            .unwrap_or_default()
    };

    let title = crate::i18n::tr("help.default_title");

    let mut body_lines = Vec::new();
    body_lines.push(crate::i18n::tr("help.description"));
    body_lines.push("".to_string());

    body_lines.push(format!("## {}", crate::i18n::tr("help.keyboard_shortcuts")));
    body_lines.push("".to_string());

    body_lines.push(format!("- **F1**: {}", crate::i18n::tr("help.key_f1")));
    body_lines.push(format!("- **F2**: {}", crate::i18n::tr("help.key_f2")));
    body_lines.push(format!("- **F3**: {}", crate::i18n::tr("help.key_f3")));
    body_lines.push(format!("- **F4**: {} (**Shift+F4**: {})", crate::i18n::tr("help.key_f4"), crate::i18n::tr("help.key_shift_f4")));
    body_lines.push(format!("- **F5**: {}", crate::i18n::tr("help.key_f5")));
    body_lines.push(format!("- **F6**: {}", crate::i18n::tr("help.key_f6")));
    body_lines.push(format!("- **F7**: {} (**Alt+F7**: {})", crate::i18n::tr("help.key_f7"), crate::i18n::tr("help.key_alt_f7")));
    body_lines.push(format!("- **F8**: {}", crate::i18n::tr("help.key_f8")));
    body_lines.push(format!("- **F9**: {}", crate::i18n::tr("help.key_f9")));
    body_lines.push(format!("- **F10**: {}", crate::i18n::tr("help.key_f10")));
    body_lines.push(format!("- **Tab**: {}", crate::i18n::tr("help.key_tab")));
    body_lines.push(format!("- **Backspace**: {}", crate::i18n::tr("help.key_backspace")));

    let refresh_keys = get_keys("refresh");
    if !refresh_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", refresh_keys, crate::i18n::tr("help.action_refresh")));
    }
    let find_keys = get_keys("find_files");
    if !find_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", find_keys, crate::i18n::tr("help.action_find_files")));
    }
    let filter_keys = get_keys("filter_files");
    if !filter_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", filter_keys, crate::i18n::tr("help.action_filter_files")));
    }
    for id in ["clip_cut", "clip_copy", "clip_paste"] {
        let keys = get_keys(id);
        if !keys.is_empty() {
            body_lines.push(format!("- **{}**: {}", keys, crate::hotkey::description(id)));
        }
    }
    let conn_keys = get_keys("manage_connections");
    if !conn_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", conn_keys, crate::i18n::tr("help.action_manage_connections")));
    }
    let save_keys = get_keys("editor_save");
    if !save_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", save_keys, crate::i18n::tr("help.action_editor_save")));
    }
    let fs_keys = get_keys("toggle_video_fullscreen");
    if !fs_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", fs_keys, crate::i18n::tr("help.action_toggle_video_fullscreen")));
    }
    let left_keys = get_keys("select_left");
    if !left_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", left_keys, crate::i18n::tr("help.action_select_left")));
    }
    let right_keys = get_keys("select_right");
    if !right_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", right_keys, crate::i18n::tr("help.action_select_right")));
    }
    let move_l_keys = get_keys("move_left");
    if !move_l_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", move_l_keys, crate::i18n::tr("help.action_move_left")));
    }
    let move_r_keys = get_keys("move_right");
    if !move_r_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", move_r_keys, crate::i18n::tr("help.action_move_right")));
    }
    let root_w_keys = get_keys("go_root_win");
    if !root_w_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", root_w_keys, crate::i18n::tr("help.action_go_root_win")));
    }
    let root_u_keys = get_keys("go_root_unix");
    if !root_u_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", root_u_keys, crate::i18n::tr("help.action_go_root_unix")));
    }

    body_lines.push("".to_string());
    body_lines.push("## Terminal".to_string());
    body_lines.push("".to_string());
    let term_keys = get_keys("expand_terminal");
    if !term_keys.is_empty() {
        body_lines.push(format!("- **{}**: {}", term_keys, crate::i18n::tr("help.action_expand_terminal")));
    }
    body_lines.push("- **Ctrl+Shift+C**: Copy selected text".to_string());
    body_lines.push("- **Ctrl+Shift+V**: Paste from clipboard".to_string());
    body_lines.push("- **Mouse drag**: Select text".to_string());
    body_lines.push("- **Right-click**: Context menu (Copy / Clear Selection / Paste)".to_string());

    body_lines.push("".to_string());
    body_lines.push(format!("## {}", crate::i18n::tr("help.navigation")));
    body_lines.push("".to_string());
    body_lines.push(crate::i18n::tr("help.navigation_text"));

    let body_text = body_lines.join("\n");
    let pango_markup = markdown_to_pango(&body_text);

    let content_vbox = Box::builder().orientation(Orientation::Vertical).build();

    let title_label = Label::builder()
        .label(&title)
        .xalign(0.0)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(12)
        .build();
    title_label.add_css_class("help-title");
    title_label.add_css_class("title-1");
    content_vbox.append(&title_label);

    let help_label = Label::builder()
        .use_markup(true)
        .selectable(true)
        .wrap(true)
        .xalign(0.0)
        .yalign(0.0)
        .margin_start(24)
        .margin_end(24)
        .margin_top(12)
        .margin_bottom(24)
        .build();
    help_label.set_markup(&pango_markup);
    content_vbox.append(&help_label);

    let scrolled = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&content_vbox)
        .build();

    root_vbox.append(&scrolled);

    let close_btn = Button::builder().label(&*crate::i18n::tr("help.close")).build();
    close_btn.set_cursor_from_name(Some("pointer"));
    let window_close = help_window.clone();
    close_btn.connect_clicked(move |_| {
        window_close.close();
    });
    header_bar.pack_end(&close_btn);

    help_window.set_content(Some(&root_vbox));
    help_window.present();
}

fn markdown_to_pango(md: &str) -> String {
    let mut pango = String::new();

    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            let text = &trimmed[2..];
            pango.push_str(&format!(
                "<span size=\"xx-large\" weight=\"bold\">{}</span>\n\n",
                escape_pango(text)
            ));
        } else if trimmed.starts_with("## ") {
            let text = &trimmed[3..];
            pango.push_str(&format!(
                "<span size=\"x-large\" weight=\"bold\">{}</span>\n\n",
                escape_pango(text)
            ));
        } else if trimmed.starts_with("### ") {
            let text = &trimmed[4..];
            pango.push_str(&format!(
                "<span size=\"large\" weight=\"bold\">{}</span>\n\n",
                escape_pango(text)
            ));
        } else if trimmed.starts_with("- ") {
            let text = &trimmed[2..];
            pango.push_str(&format!(
                "  • {}\n",
                parse_inline_markdown(&escape_pango(text))
            ));
        } else if trimmed.starts_with("* ") {
            let text = &trimmed[2..];
            pango.push_str(&format!(
                "  • {}\n",
                parse_inline_markdown(&escape_pango(text))
            ));
        } else if trimmed.is_empty() {
            pango.push('\n');
        } else {
            pango.push_str(&format!(
                "{}\n",
                parse_inline_markdown(&escape_pango(trimmed))
            ));
        }
    }
    pango
}

fn escape_pango(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

fn parse_inline_markdown(text: &str) -> String {
    let mut result = text.to_string();

    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let end = start + 2 + end;
            let bold_text = &result[start + 2..end];
            result = format!(
                "{}<b>{}</b>{}",
                &result[..start],
                bold_text,
                &result[end + 2..]
            );
        } else {
            break;
        }
    }

    while let Some(start) = result.find('*') {
        if let Some(end) = result[start + 1..].find('*') {
            let end = start + 1 + end;
            let italic_text = &result[start + 1..end];
            result = format!(
                "{}<i>{}</i>{}",
                &result[..start],
                italic_text,
                &result[end + 1..]
            );
        } else {
            break;
        }
    }

    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let end = start + 1 + end;
            let code_text = &result[start + 1..end];
            result = format!("{}<span font_family=\"monospace\" background=\"alpha(currentColor,0.07)\">{}</span>{}", &result[..start], code_text, &result[end + 1..]);
        } else {
            break;
        }
    }

    result
}
