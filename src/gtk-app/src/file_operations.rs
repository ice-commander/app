use adw::prelude::*;

use transfer_core::{
    execute_transfer, execute_transfer_offthread, format_size, mode_of, scan_items, CopyMessage,
    ErrorAction, ErrorRequest, TransferItem,
};
use gtk::glib;
use fm_core::rpc::FileSystemRpc;

use common::AppError;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;



pub fn trigger_copy(
    window: &adw::ApplicationWindow,
    active_fm: &Rc<panel_router::PanelRouter>,
    inactive_fm: &Rc<panel_router::PanelRouter>,
) {
    show_transfer_dialog(window, active_fm, inactive_fm, false);
}

pub fn trigger_move(
    window: &adw::ApplicationWindow,
    active_fm: &Rc<panel_router::PanelRouter>,
    inactive_fm: &Rc<panel_router::PanelRouter>,
) {
    show_transfer_dialog(window, active_fm, inactive_fm, true);
}

fn show_error_dialog(window: &adw::ApplicationWindow, title: &str, message: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(message)
        .build();
    dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
    dialog.connect_response(None, move |d, _| {
        d.close();
    });
    dialog.present(Some(window));
}


fn get_selected_items_info(fm: &Rc<panel_router::PanelRouter>) -> Vec<(String, bool, u64, Option<u32>)> {
    fm.selected_entries()
        .into_iter()
        .filter(|entry| entry.name() != "..")
        .map(|entry| (entry.name(), entry.is_dir(), entry.size(), entry.permissions()))
        .collect()
}

fn show_transfer_dialog(
    window: &adw::ApplicationWindow,
    active_fm: &Rc<panel_router::PanelRouter>,
    inactive_fm: &Rc<panel_router::PanelRouter>,
    is_move: bool,
) {
    let selected_items = get_selected_items_info(active_fm);

    let action_name = if is_move { "move" } else { "copy" };

    if selected_items.is_empty() {
        let err_msg = if is_move {
            crate::i18n::tr("fm.no_selection_move")
        } else {
            crate::i18n::tr("fm.no_selection_copy")
        };
        show_error_dialog(window, &crate::i18n::tr("fm.no_selection"), &err_msg);
        return;
    }

    let src_parent = active_fm.current_path_string();
    let dest_parent = inactive_fm.current_path_string();

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .width_request(450)
        .build();

    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Start)
        .build();

    let title_icon_res = if is_move {
        "/com/icecommander/gtk/cut.svg"
    } else {
        "/com/icecommander/gtk/copy.svg"
    };
    let title_icon = gtk::Image::from_resource(title_icon_res);
    title_icon.set_pixel_size(24);
    title_box.append(&title_icon);

    let title_text = if is_move {
        crate::i18n::tr("fm.move_files_title")
    } else {
        crate::i18n::tr("fm.copy_files_title")
    };
    let title_label = gtk::Label::builder()
        .label(&format!(
            "<span weight='bold' size='large'>{}</span>",
            title_text
        ))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    title_box.append(&title_label);
    content_box.append(&title_box);

    let from_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let from_label = gtk::Label::builder()
        .label(&format!("<b>{}</b>", crate::i18n::tr("fm.from")))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    from_vbox.append(&from_label);

    let src_path_label = gtk::Label::builder()
        .label(&format!("<span color='gray'>{}</span>", src_parent))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    from_vbox.append(&src_path_label);
    content_box.append(&from_vbox);

    let to_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let to_label = gtk::Label::builder()
        .label(&format!("<b>{}</b>", crate::i18n::tr("fm.to")))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    to_vbox.append(&to_label);

    let entry = gtk::Entry::builder()
        .text(&dest_parent)
        .activates_default(true)
        .build();
    to_vbox.append(&entry);
    content_box.append(&to_vbox);

    let separator = gtk::Separator::builder()
        .orientation(gtk::Orientation::Horizontal)
        .build();
    content_box.append(&separator);

    let scan_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let scan_label = gtk::Label::builder()
        .label(&*crate::i18n::tr("fm.scanning_items"))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    scan_box.append(&scan_label);

    let spinner = gtk::Spinner::new();
    spinner.start();
    scan_box.append(&spinner);
    content_box.append(&scan_box);

    let dialog = adw::AlertDialog::builder()
        .extra_child(&content_box)
        .build();

    dialog.add_response("cancel", &*crate::i18n::tr("fm.cancel"));
    dialog.add_response(action_name, &*crate::i18n::tr(if is_move { "fm.action_move" } else { "fm.action_copy" }));
    dialog.set_default_response(Some(action_name));
    dialog.set_response_appearance(action_name, adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled(action_name, false);

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let cancel_tx_cell = Rc::new(std::cell::RefCell::new(Some(cancel_tx)));
    let cancel_tx_cell_c = cancel_tx_cell.clone();

    let cancellation_flag = Arc::new(AtomicBool::new(false));
    let cancellation_flag_c = cancellation_flag.clone();

    let scan_label_c = scan_label.clone();
    let scan_label_final = scan_label.clone();
    let spinner_c = spinner.clone();
    let dialog_c = dialog.clone();
    let action_name_str = action_name.to_string();

    let src_provider = active_fm.provider();

    let scan_future = scan_items(
        src_provider,
        selected_items.clone(),
        src_parent.clone(),
        move |files, dirs, bytes| {
            let size_str = format_size(bytes);
            scan_label_c.set_text(&crate::i18n::trf("fm.scale_status", &[("files", &*(files.to_string()).to_string()), ("dirs", &*(dirs.to_string()).to_string()), ("size", &*(size_str).to_string())]));
        },
        cancellation_flag_c.clone(),
    );

    let items_cell = Rc::new(std::cell::RefCell::new(Vec::new()));
    let items_cell_c = items_cell.clone();

    gtk::glib::spawn_future_local(async move {
        tokio::select! {
            res = scan_future => {
                match res {
                    Ok(scanned_items) => {
                        spinner_c.stop();
                        spinner_c.set_visible(false);
                        
                        let mut files = 0;
                        let mut dirs = 0;
                        let mut bytes = 0;
                        for item in &scanned_items {
                            if item.is_dir {
                                dirs += 1;
                            } else {
                                files += 1;
                                bytes += item.size;
                            }
                        }
                        let size_str = format_size(bytes);
                        scan_label_final.set_text(&crate::i18n::trf("fm.scale_status", &[("files", &*(files.to_string()).to_string()), ("dirs", &*(dirs.to_string()).to_string()), ("size", &*(size_str).to_string())]));

                        dialog_c.set_response_enabled(&action_name_str, true);
                        *items_cell_c.borrow_mut() = scanned_items;
                    }
                    Err(_) => {}
                }
            }
            _ = cancel_rx => {
            }
        }
    });

    let active_fm_c = active_fm.clone();
    let inactive_fm_c = inactive_fm.clone();
    let window_c = window.clone();
    let cancellation_flag_response = cancellation_flag.clone();
    let items_cell_response = items_cell.clone();

    dialog.connect_response(None, move |d, response| {
        if let Some(tx) = cancel_tx_cell_c.borrow_mut().take() {
            let _ = tx.send(());
        }
        if response == action_name {
            let dest_path_str = entry.text().to_string();
            let items = items_cell_response.borrow().clone();
            if !dest_path_str.is_empty() && !items.is_empty() {
                show_progress_dialog(
                    &window_c,
                    active_fm_c.clone(),
                    inactive_fm_c.clone(),
                    items,
                    src_parent.clone(),
                    dest_path_str,
                    selected_items.clone(),
                    is_move,
                );
            }
        } else {
            cancellation_flag_response.store(true, Ordering::Relaxed);
        }
        d.close();
    });
    dialog.present(Some(window));
}

enum OverwriteDecision {
    Yes,
    YesToAll,
    Skip,
    SkipAll,
    Cancel,
}

async fn resolve_conflicts(
    window: &adw::ApplicationWindow,
    dest_provider: &std::rc::Rc<dyn FileSystemRpc>,
    dest_parent: &str,
    items: Vec<TransferItem>,
) -> Option<Vec<TransferItem>> {
    let existing: std::collections::HashSet<String> = match dest_provider.list_dir(dest_parent.to_string()).await {
        Ok(entries) => entries.into_iter().map(|e| e.name).collect(),
        Err(_) => return Some(items),
    };

    let top_level = |rel: &str| rel.split('/').next().unwrap_or(rel).to_string();

    let mut conflicts: Vec<String> = Vec::new();
    for item in &items {
        let top = top_level(&item.relative_path);
        if existing.contains(&top) && !conflicts.contains(&top) {
            conflicts.push(top);
        }
    }
    if conflicts.is_empty() {
        return Some(items);
    }

    let mut skip: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut apply_all: Option<bool> = None;

    for (idx, name) in conflicts.iter().enumerate() {
        let overwrite = match apply_all {
            Some(v) => v,
            None => match ask_overwrite_each(window, name, conflicts.len() - idx).await {
                OverwriteDecision::Yes => true,
                OverwriteDecision::YesToAll => {
                    apply_all = Some(true);
                    true
                }
                OverwriteDecision::Skip => false,
                OverwriteDecision::SkipAll => {
                    apply_all = Some(false);
                    false
                }
                OverwriteDecision::Cancel => return None,
            },
        };
        if !overwrite {
            skip.insert(name.clone());
        }
    }

    if skip.is_empty() {
        Some(items)
    } else {
        Some(
            items
                .into_iter()
                .filter(|it| !skip.contains(&top_level(&it.relative_path)))
                .collect(),
        )
    }
}

async fn ask_overwrite_each(
    window: &adw::ApplicationWindow,
    name: &str,
    remaining: usize,
) -> OverwriteDecision {
    let dialog = adw::AlertDialog::builder()
        .heading("File exists")
        .body(&format!("\"{name}\" already exists at the destination.\nOverwrite it?"))
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("skip", "Skip");
    if remaining > 1 {
        dialog.add_response("skip_all", "Skip All");
        dialog.add_response("yes_all", "Yes to All");
    }
    dialog.add_response("yes", "Yes");
    dialog.set_default_response(Some("yes"));
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Suggested);

    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Rc::new(std::cell::RefCell::new(Some(tx)));
    dialog.connect_response(None, move |d, resp| {
        let decision = match resp {
            "yes" => OverwriteDecision::Yes,
            "yes_all" => OverwriteDecision::YesToAll,
            "skip" => OverwriteDecision::Skip,
            "skip_all" => OverwriteDecision::SkipAll,
            _ => OverwriteDecision::Cancel,
        };
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send(decision);
        }
        d.close();
    });
    dialog.present(Some(window));

    rx.await.unwrap_or(OverwriteDecision::Cancel)
}

fn run_transfer_with_progress(
    window: &adw::ApplicationWindow,
    src_provider: std::rc::Rc<dyn FileSystemRpc>,
    dest_provider: std::rc::Rc<dyn FileSystemRpc>,
    remaining_items: Vec<TransferItem>,
    dest_parent: String,
    is_move: bool,
    on_finish: impl Fn() + 'static,
) {
    let window = window.clone();

    gtk::glib::spawn_future_local(async move {
        let remaining_items = match resolve_conflicts(&window, &dest_provider, &dest_parent, remaining_items).await {
            Some(items) => items,
            None => {
                on_finish();
                return;
            }
        };
        if remaining_items.is_empty() {
            on_finish();
            return;
        }

        let action_title = if is_move {
            crate::i18n::tr("fm.moving_files")
        } else {
            crate::i18n::tr("fm.copying_files")
        };

        let progress_dialog = adw::AlertDialog::builder()
            .heading(action_title)
            .build();

        let progress_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .width_request(450)
            .build();

        let current_file_label = gtk::Label::builder()
            .label(&*crate::i18n::tr("fm.starting_transfer"))
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        progress_box.append(&current_file_label);

        let current_file_progress = gtk::ProgressBar::builder().hexpand(true).build();
        progress_box.append(&current_file_progress);

        let overall_label = gtk::Label::builder()
            .label(&*crate::i18n::tr("fm.overall_progress_start"))
            .halign(gtk::Align::Start)
            .build();
        progress_box.append(&overall_label);

        let overall_progress = gtk::ProgressBar::builder().hexpand(true).build();
        progress_box.append(&overall_progress);

        progress_dialog.set_extra_child(Some(&progress_box));
        progress_dialog.add_response("cancel", &*crate::i18n::tr("fm.cancel"));

        let cancellation_flag = Arc::new(AtomicBool::new(false));
        let cancellation_flag_c = cancellation_flag.clone();

        let current_file_label_c2 = current_file_label.clone();
        progress_dialog.connect_response(None, move |_d, response| {
            if response == "cancel" {
                cancellation_flag_c.store(true, Ordering::Relaxed);
                current_file_label_c2.set_text(&crate::i18n::tr("fm.cancelling_transfer"));
            }
        });

        progress_dialog.present(Some(&window));

        let total_bytes: u64 = remaining_items
            .iter()
            .filter(|i| !i.is_dir)
            .map(|i| i.size)
            .sum();
        let total_files = remaining_items.iter().filter(|i| !i.is_dir).count();

        let current_file_label_c = current_file_label.clone();
        let current_file_progress_c = current_file_progress.clone();
        let overall_label_c = overall_label.clone();
        let overall_progress_c = overall_progress.clone();

        let (progress_tx, progress_rx) = std::sync::mpsc::channel::<CopyMessage>();
        let (error_tx, mut error_rx) = tokio::sync::mpsc::unbounded_channel::<ErrorRequest>();

        let api_op = crate::api::ops_begin(
            if is_move { "move" } else { "copy" },
            total_bytes,
            total_files,
            cancellation_flag.clone(),
        );

        let timer_window = window.clone();
        let error_pending = Rc::new(std::cell::Cell::new(false));

        let mut current_file_name = crate::i18n::tr("fm.starting_transfer");
        let mut current_file_size = 0u64;
        let mut current_file_copied = 0u64;
        let mut overall_copied_bytes = 0u64;
        let mut files_copied = 0usize;
        let mut state_changed = true;

        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let mut disconnected = false;

            loop {
                match progress_rx.try_recv() {
                    Ok(msg) => {
                        state_changed = true;
                        match msg {
                            CopyMessage::FileStart { name, size } => {
                                current_file_name = name;
                                current_file_size = size;
                                current_file_copied = 0;
                            }
                            CopyMessage::Progress { file_bytes, total_bytes_copied } => {
                                current_file_copied = file_bytes;
                                overall_copied_bytes = total_bytes_copied;
                            }
                            CopyMessage::FileDone => {
                                files_copied += 1;
                                current_file_copied = current_file_size;
                            }
                            CopyMessage::FileSkipped => {
                                files_copied += 1;
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if state_changed && !disconnected {
                api_op.update(&current_file_name, overall_copied_bytes, files_copied);
                current_file_label_c.set_text(&crate::i18n::trf("fm.transferring_file", &[("file", &*(current_file_name).to_string())]));
                if current_file_size > 0 {
                    let frac = (current_file_copied as f64 / current_file_size as f64).clamp(0.0, 1.0);
                    current_file_progress_c.set_fraction(frac);
                } else {
                    current_file_progress_c.set_fraction(1.0);
                }

                if total_bytes > 0 {
                    let overall_frac = (overall_copied_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);
                    overall_progress_c.set_fraction(overall_frac);

                    let copied_size_str = format_size(overall_copied_bytes);
                    let total_size_str = format_size(total_bytes);

                    overall_label_c.set_text(&crate::i18n::trf("fm.overall_progress_stats", &[("copied_size", &*(copied_size_str).to_string()), ("total_size", &*(total_size_str).to_string()), ("files_copied", &*(files_copied.to_string()).to_string()), ("total_files", &*(total_files.to_string()).to_string())]));
                } else {
                    overall_progress_c.set_fraction(1.0);
                    overall_label_c.set_text(&crate::i18n::trf("fm.overall_progress_files", &[("files_copied", &*(files_copied.to_string()).to_string()), ("total_files", &*(total_files.to_string()).to_string())]));
                }
                state_changed = false;
            }

            if !error_pending.get() {
                if let Ok(req) = error_rx.try_recv() {
                    error_pending.set(true);
                    let err_dialog = adw::AlertDialog::builder()
                        .heading("Transfer Error")
                        .body(&format!("{}\n\n{}", req.file, req.message))
                        .build();
                    err_dialog.add_response("abort", "Cancel");
                    err_dialog.add_response("skip", "Skip");
                    err_dialog.add_response("retry", "Retry");
                    err_dialog.set_default_response(Some("retry"));
                    err_dialog.set_response_appearance("retry", adw::ResponseAppearance::Suggested);

                    let reply = Rc::new(std::cell::RefCell::new(Some(req.reply)));
                    let error_pending_c = error_pending.clone();
                    err_dialog.connect_response(None, move |d, resp| {
                        let action = match resp {
                            "retry" => ErrorAction::Retry,
                            "skip" => ErrorAction::Skip,
                            _ => ErrorAction::Abort,
                        };
                        if let Some(tx) = reply.borrow_mut().take() {
                            let _ = tx.send(action);
                        }
                        error_pending_c.set(false);
                        d.close();
                    });
                    err_dialog.present(Some(&timer_window));
                }
            }

            if disconnected {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });

        let plans = crate::transfer_plan::EndpointPlan::of(&src_provider)
            .zip(crate::transfer_plan::EndpointPlan::of(&dest_provider));
        let res = match plans {
            Some((src_plan, dest_plan)) => {
                execute_transfer_offthread(
                    src_plan.into_factory(),
                    dest_plan.into_factory(),
                    remaining_items,
                    dest_parent,
                    is_move,
                    progress_tx,
                    error_tx,
                    cancellation_flag.clone(),
                )
                .await
            }
            None => {
                execute_transfer(
                    src_provider,
                    dest_provider,
                    remaining_items,
                    dest_parent,
                    is_move,
                    progress_tx,
                    error_tx,
                    cancellation_flag.clone(),
                )
                .await
            }
        };

        progress_dialog.close();

        if let Err(e) = &res {
            if *e != AppError::Cancelled {
                show_error_dialog(&window, &crate::i18n::tr("fm.transfer_error"), &e.to_string());
            }
        }
        on_finish();
    });
}

fn show_progress_dialog(
    window: &adw::ApplicationWindow,
    active_fm: Rc<panel_router::PanelRouter>,
    inactive_fm: Rc<panel_router::PanelRouter>,
    items: Vec<TransferItem>,
    src_parent: String,
    dest_parent: String,
    selected_items_info: Vec<(String, bool, u64, Option<u32>)>,
    is_move: bool,
) {
    let src_provider = active_fm.provider();
    let dest_provider = inactive_fm.provider();

    let src_is_local = src_provider.is_local() && !items.is_empty();
    let dest_is_local = dest_provider.is_local();

    let mut remaining_items = items;

    if is_move && src_is_local && dest_is_local {
        let top_levels: Vec<String> = selected_items_info
            .iter()
            .map(|(name, _, _, _)| name.clone())
            .collect();

        let mut failed_top_levels = Vec::new();
        for tl_name in top_levels {
            let src_path = std::path::Path::new(&src_parent).join(&tl_name);
            let dest_path = std::path::Path::new(&dest_parent).join(&tl_name);

            if let Some(parent) = dest_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            if std::fs::rename(&src_path, &dest_path).is_err() {
                failed_top_levels.push(tl_name);
            }
        }

        if failed_top_levels.is_empty() {
            active_fm.refresh_spawned();
            inactive_fm.refresh_spawned();
            return;
        } else {
            remaining_items.retain(|item| {
                failed_top_levels.iter().any(|ftl| {
                    item.relative_path == *ftl
                        || item.relative_path.starts_with(&format!("{}/", ftl))
                })
            });
        }
    }

    if remaining_items.is_empty() {
        active_fm.refresh_spawned();
        inactive_fm.refresh_spawned();
        return;
    }

    let active_fm_c = active_fm.clone();
    let inactive_fm_c = inactive_fm.clone();
    run_transfer_with_progress(
        window,
        src_provider,
        dest_provider,
        remaining_items,
        dest_parent,
        is_move,
        move || {
            active_fm_c.refresh_spawned();
            inactive_fm_c.refresh_spawned();
        },
    );
}

pub fn trigger_external_drop(
    window: &adw::ApplicationWindow,
    src_paths: Vec<std::path::PathBuf>,
    dest_fm: &Rc<panel_router::PanelRouter>,
    source_fm: Option<Rc<panel_router::PanelRouter>>,
) {
    show_external_drop_transfer_dialog(window, src_paths, dest_fm, source_fm);
}

fn show_external_drop_transfer_dialog(
    window: &adw::ApplicationWindow,
    src_paths: Vec<std::path::PathBuf>,
    dest_fm: &Rc<panel_router::PanelRouter>,
    source_fm: Option<Rc<panel_router::PanelRouter>>,
) {
    let selected_items: Vec<(String, bool, u64, Option<u32>)> = src_paths
        .iter()
        .map(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let meta = std::fs::metadata(path).ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let permissions = meta.as_ref().and_then(mode_of);
            (name, is_dir, size, permissions)
        })
        .collect();

    if selected_items.is_empty() {
        return;
    }

    let src_parent = if let Some(first_path) = src_paths.first() {
        if let Some(parent) = first_path.parent() {
            parent.to_string_lossy().to_string()
        } else {
            "/".to_string()
        }
    } else {
        return;
    };

    let dest_parent = dest_fm.current_path_string();

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .width_request(450)
        .build();

    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Start)
        .build();

    let title_icon = gtk::Image::from_resource("/com/icecommander/gtk/copy.svg");
    title_icon.set_pixel_size(24);
    title_box.append(&title_icon);

    let title_label = gtk::Label::builder()
        .label(&format!(
            "<span weight='bold' size='large'>{}</span>",
            crate::i18n::tr("fm.copy_files_title")
        ))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    title_box.append(&title_label);
    content_box.append(&title_box);

    let from_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let from_label = gtk::Label::builder()
        .label(&format!("<b>{}</b>", crate::i18n::tr("fm.from")))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    from_vbox.append(&from_label);

    let src_path_label = gtk::Label::builder()
        .label(&format!("<span color='gray'>{}</span>", src_parent))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    from_vbox.append(&src_path_label);
    content_box.append(&from_vbox);

    let to_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let to_label = gtk::Label::builder()
        .label(&format!("<b>{}</b>", crate::i18n::tr("fm.to")))
        .use_markup(true)
        .halign(gtk::Align::Start)
        .build();
    to_vbox.append(&to_label);

    let entry = gtk::Entry::builder()
        .text(&dest_parent)
        .activates_default(true)
        .build();
    to_vbox.append(&entry);
    content_box.append(&to_vbox);

    let separator = gtk::Separator::builder()
        .orientation(gtk::Orientation::Horizontal)
        .build();
    content_box.append(&separator);

    let scan_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let scan_label = gtk::Label::builder()
        .label(&*crate::i18n::tr("fm.scanning_items"))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    scan_box.append(&scan_label);

    let spinner = gtk::Spinner::new();
    spinner.start();
    scan_box.append(&spinner);
    content_box.append(&scan_box);

    let dialog = adw::AlertDialog::builder()
        .extra_child(&content_box)
        .build();

    dialog.add_response("cancel", &*crate::i18n::tr("fm.cancel"));
    dialog.add_response("copy", &*crate::i18n::tr("fm.action_copy"));
    dialog.set_default_response(Some("copy"));
    dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("copy", false);

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let cancel_tx_cell = Rc::new(std::cell::RefCell::new(Some(cancel_tx)));
    let cancel_tx_cell_c = cancel_tx_cell.clone();

    let cancellation_flag = Arc::new(AtomicBool::new(false));
    let cancellation_flag_c = cancellation_flag.clone();

    let scan_label_c = scan_label.clone();
    let scan_label_final = scan_label.clone();
    let spinner_c = spinner.clone();
    let dialog_c = dialog.clone();

    let src_provider = std::rc::Rc::new(virtualfs::local_rpc::LocalFileSystemRpc::new(dest_fm.config()));

    let scan_future = scan_items(
        src_provider,
        selected_items.clone(),
        src_parent.clone(),
        move |files, dirs, bytes| {
            let size_str = format_size(bytes);
            scan_label_c.set_text(&crate::i18n::trf("fm.scale_status", &[("files", &*(files.to_string()).to_string()), ("dirs", &*(dirs.to_string()).to_string()), ("size", &*(size_str).to_string())]));
        },
        cancellation_flag_c.clone(),
    );

    let items_cell = Rc::new(std::cell::RefCell::new(Vec::new()));
    let items_cell_c = items_cell.clone();

    gtk::glib::spawn_future_local(async move {
        tokio::select! {
            res = scan_future => {
                match res {
                    Ok(scanned_items) => {
                        spinner_c.stop();
                        spinner_c.set_visible(false);
                        
                        let mut files = 0;
                        let mut dirs = 0;
                        let mut bytes = 0;
                        for item in &scanned_items {
                            if item.is_dir {
                                dirs += 1;
                            } else {
                                files += 1;
                                bytes += item.size;
                            }
                        }
                        let size_str = format_size(bytes);
                        scan_label_final.set_text(&crate::i18n::trf("fm.scale_status", &[("files", &*(files.to_string()).to_string()), ("dirs", &*(dirs.to_string()).to_string()), ("size", &*(size_str).to_string())]));

                        dialog_c.set_response_enabled("copy", true);
                        *items_cell_c.borrow_mut() = scanned_items;
                    }
                    Err(_) => {}
                }
            }
            _ = cancel_rx => {
            }
        }
    });

    let dest_fm_c = dest_fm.clone();
    let source_fm_c = source_fm.clone();
    let window_c = window.clone();
    let cancellation_flag_response = cancellation_flag.clone();
    let items_cell_response = items_cell.clone();

    dialog.connect_response(None, move |d, response| {
        if let Some(tx) = cancel_tx_cell_c.borrow_mut().take() {
            let _ = tx.send(());
        }
        if response == "copy" {
            let dest_path_str = entry.text().to_string();
            let items = items_cell_response.borrow().clone();
            if !dest_path_str.is_empty() && !items.is_empty() {
                show_external_drop_progress_dialog(
                    &window_c,
                    dest_fm_c.clone(),
                    source_fm_c.clone(),
                    items,
                    src_parent.clone(),
                    dest_path_str,
                    selected_items.clone(),
                );
            }
        } else {
            cancellation_flag_response.store(true, Ordering::Relaxed);
        }
        d.close();
    });
    dialog.present(Some(window));
}

fn show_external_drop_progress_dialog(
    window: &adw::ApplicationWindow,
    dest_fm: Rc<panel_router::PanelRouter>,
    source_fm: Option<Rc<panel_router::PanelRouter>>,
    items: Vec<TransferItem>,
    _src_parent: String,
    dest_parent: String,
    _selected_items_info: Vec<(String, bool, u64, Option<u32>)>,
) {
    let src_provider = std::rc::Rc::new(virtualfs::local_rpc::LocalFileSystemRpc::new(dest_fm.config()));
    let dest_provider = dest_fm.provider();

    let remaining_items = items;

    if remaining_items.is_empty() {
        dest_fm.refresh_spawned();
        if let Some(ref sfm) = source_fm {
            sfm.refresh_spawned();
        }
        return;
    }

    run_transfer_with_progress(
        window,
        src_provider,
        dest_provider,
        remaining_items,
        dest_parent,
        false,
        move || {
            dest_fm.refresh_spawned();
            if let Some(ref sfm) = source_fm {
                sfm.refresh_spawned();
            }
        },
    );
}
