macro_rules! println {
    ($($arg:tt)*) => {
        if false {
            let _ = format_args!($($arg)*);
        }
    };
}

use adw::prelude::*;
use gtk::glib;
use gtk::{
    Align, Box, Button, Entry, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
};
use fm_core::rpc::FileSystemRpc;
use panel_router::PanelRouter;
use std::cell::RefCell;

use common::AppError;
use std::rc::Rc;

async fn list_dir_async(
    provider: Rc<dyn FileSystemRpc>,
    path: String,
) -> Result<(Vec<String>, Vec<(String, u64)>), AppError> {
    println!("[SEARCH RPC] list_dir_async called for path: '{}'", path);
    let path_for_log = path.clone();
    let entries = provider.list_dir(path).await.map_err(AppError::from)?;
    println!("[SEARCH RPC] list_dir completed for '{}'", path_for_log);
    let dirs = entries.iter().filter(|e| e.is_dir).map(|e| e.name.clone()).collect();
    let files = entries.iter().filter(|e| !e.is_dir).map(|e| (e.name.clone(), e.size)).collect();
    Ok((dirs, files))
}

fn join_paths(parent: &str, child: &str) -> String {
    let mut p = parent.to_string();
    if !p.ends_with('/') && !p.ends_with('\\') {
        p.push('/');
    }
    p.push_str(child);
    p
}

fn format_size(size: u64) -> String {
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

struct SearchResult {
    name: String,
    size: u64,
    date: String,
    folder: String,
}

enum SearchMessage {
    Match(SearchResult),
    ScanningDir(String),
    Finished(bool), // true if capped, false if completed
}

pub fn show_search_dialog<P: glib::object::IsA<gtk::Window>>(parent: &P, active_fm: &Rc<PanelRouter>) {
    let current_path_str = active_fm.current_path_string();
    println!(
        "[SEARCH UI] show_search_dialog called. current_path_str: '{}'",
        current_path_str
    );
    let provider_opt = Some(active_fm.provider());
    if provider_opt.is_none() {
        println!("[SEARCH UI] Warning: active file manager has no provider.");
    }

    let dialog = gtk::Window::builder()
        .title(&*crate::i18n::tr("find.title"))
        .transient_for(parent)
        .modal(true)
        .default_width(750)
        .default_height(550)
        .build();

    let main_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(16)
        .margin_bottom(16)
        .build();

    let path_label = Label::builder()
        .label(&*crate::i18n::trf("find.starting_from", &[("path", &*(current_path_str).to_string())]))
        .use_markup(true)
        .halign(Align::Start)
        .css_classes(vec!["dim-label"])
        .build();
    main_box.append(&path_label);

    let input_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let search_entry = Entry::builder()
        .placeholder_text(&*crate::i18n::tr("find.query_placeholder"))
        .hexpand(true)
        .build();
    input_box.append(&search_entry);

    let search_btn = Button::builder()
        .label(&*crate::i18n::tr("find.search_btn"))
        .css_classes(vec!["suggested-action"])
        .build();
    input_box.append(&search_btn);

    main_box.append(&input_box);

    let header_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(4)
        .build();

    let lbl_name_hdr = Label::builder()
        .label(&*crate::i18n::tr("find.hdr_name"))
        .halign(Align::Start)
        .css_classes(vec!["bold"])
        .build();
    let lbl_size_hdr = Label::builder()
        .label(&*crate::i18n::tr("find.hdr_size"))
        .halign(Align::Start)
        .css_classes(vec!["bold"])
        .build();
    let lbl_date_hdr = Label::builder()
        .label(&*crate::i18n::tr("find.hdr_date"))
        .halign(Align::Start)
        .css_classes(vec!["bold"])
        .build();
    let lbl_folder_hdr = Label::builder()
        .label(&*crate::i18n::tr("find.hdr_location"))
        .halign(Align::Start)
        .css_classes(vec!["bold"])
        .hexpand(true)
        .build();

    lbl_name_hdr.set_width_request(180);
    lbl_size_hdr.set_width_request(80);
    lbl_date_hdr.set_width_request(140);

    header_box.append(&lbl_name_hdr);
    header_box.append(&lbl_size_hdr);
    header_box.append(&lbl_date_hdr);
    header_box.append(&lbl_folder_hdr);

    main_box.append(&header_box);

    let separator = gtk::Separator::new(Orientation::Horizontal);
    main_box.append(&separator);

    let scrolled_win = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .propagate_natural_height(false)
        .build();

    let list_box = ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(false)
        .build();
    scrolled_win.set_child(Some(&list_box));
    main_box.append(&scrolled_win);

    let status_label = Label::builder()
        .label(&*crate::i18n::tr("find.status_ready"))
        .halign(Align::Start)
        .css_classes(vec!["dim-label"])
        .build();
    main_box.append(&status_label);

    dialog.set_child(Some(&main_box));

    let searching = Rc::new(RefCell::new(false));
    let cancel_flag = Rc::new(RefCell::new(false));
    let destroyed = Rc::new(RefCell::new(false));
    let result_paths = Rc::new(RefCell::new(Vec::<String>::new()));
    let thread_cancel_flag = Rc::new(RefCell::new(
        None::<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ));

    let destroyed_c = destroyed.clone();
    let cancel_flag_c = cancel_flag.clone();
    let thread_cancel_c = thread_cancel_flag.clone();
    dialog.connect_destroy(move |_| {
        println!("[SEARCH UI] Search dialog destroyed. Cancelling active search if running.");
        *destroyed_c.borrow_mut() = true;
        *cancel_flag_c.borrow_mut() = true;
        if let Some(flag) = thread_cancel_c.borrow().as_ref() {
            println!("[SEARCH UI] Signaling background thread cancel flag.");
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });

    let fm_clone = active_fm.clone();
    let dialog_clone = dialog.clone();
    let paths_clone = result_paths.clone();
    list_box.connect_row_activated(move |_, row| {
        let idx = row.index() as usize;
        println!("[SEARCH UI] Row activated at index {}.", idx);
        if let Some(folder_path) = paths_clone.borrow().get(idx) {
            println!(
                "[SEARCH UI] Requesting file manager to navigate to folder: '{}'",
                folder_path
            );
            fm_clone.open_path(folder_path.clone());
            dialog_clone.close();
        } else {
            println!(
                "[SEARCH UI] Error: Could not find folder path for row index {}.",
                idx
            );
        }
    });

    let search_btn_clone = search_btn.clone();
    let search_entry_clone = search_entry.clone();
    let list_box_clone = list_box.clone();
    let status_lbl_clone = status_label.clone();
    let searching_clone = searching.clone();
    let cancel_flag_clone = cancel_flag.clone();
    let result_paths_clone = result_paths.clone();
    let destroyed_clone = destroyed.clone();

    let thread_cancel_flag_clone = thread_cancel_flag.clone();
    let dialog_for_search = dialog.clone();
    let active_fm_search_clone = active_fm.clone();

    let trigger_search = move || {
        let is_searching = *searching_clone.borrow();
        if is_searching {
            println!("[SEARCH UI] Stop button clicked. Setting cancel flags.");
            *cancel_flag_clone.borrow_mut() = true;
            if let Some(flag) = thread_cancel_flag_clone.borrow().as_ref() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            search_btn_clone.set_sensitive(false);
            search_btn_clone.set_label(&*crate::i18n::tr("find.stopping_btn"));
        } else {
            let query = search_entry_clone.text().to_string();
            if query.trim().is_empty() {
                return;
            }
            let skip_hidden =
                !active_fm_search_clone.config().get::<bool>(&active_fm_search_clone.show_hidden_config_key())
                    .unwrap_or(false);
            println!(
                "[SEARCH UI] Starting new search | Query: '{}' | Path: '{}'",
                query, current_path_str
            );

            let provider = match provider_opt.clone() {
                Some(p) => p,
                None => {
                    status_lbl_clone.set_text("Error: No active directory provider.");
                    return;
                }
            };

            *searching_clone.borrow_mut() = true;
            *cancel_flag_clone.borrow_mut() = false;
            search_btn_clone.set_label(&*crate::i18n::tr("find.stop_btn"));
            search_btn_clone.add_css_class("destructive-action");
            search_btn_clone.remove_css_class("suggested-action");

            while let Some(child) = list_box_clone.first_child() {
                list_box_clone.remove(&child);
            }
            result_paths_clone.borrow_mut().clear();

            let list_box_inner = list_box_clone.clone();
            let status_lbl_inner = status_lbl_clone.clone();
            let btn_inner = search_btn_clone.clone();
            let searching_inner = searching_clone.clone();
            let cancel_flag_inner = cancel_flag_clone.clone();
            let paths_inner = result_paths_clone.clone();
            let start_dir = current_path_str.clone();
            let destroyed_inner = destroyed_clone.clone();

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SearchMessage>();
            let atomic_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            *thread_cancel_flag.borrow_mut() = Some(atomic_cancel.clone());

            if provider.is_local() {
                let tx_bg = tx.clone();
                let atomic_cancel_bg = atomic_cancel.clone();
                let start_dir_bg = start_dir.clone();
                let query_bg = query.clone();

                std::thread::spawn(move || {
                    println!("[SEARCH THREAD] Background search thread started | Query: '{}' | Path: '{}'", query_bg, start_dir_bg);

                    let mut queue = vec![start_dir_bg];
                    let mut visited = std::collections::HashSet::new();
                    let mut visited_count = 0;
                    let mut found_count = 0;
                    let query_lower = query_bg.to_lowercase();

                    while let Some(current_dir) = queue.pop() {
                        let is_logged = visited_count % 100 == 0;
                        if is_logged {
                            println!("[SEARCH THREAD] Popping directory from queue: '{}'. Remaining queue size: {}", current_dir, queue.len());
                        }
                        if atomic_cancel_bg.load(std::sync::atomic::Ordering::Relaxed) {
                            println!(
                                "[SEARCH THREAD] Stop/cancel signal detected. Exiting thread."
                            );
                            break;
                        }

                        let canon_start = std::time::Instant::now();
                        let canon_res = std::fs::canonicalize(&current_dir);
                        let canon_elapsed = canon_start.elapsed();
                        if is_logged || canon_elapsed.as_millis() > 5 {
                            println!(
                                "[SEARCH THREAD] Canonicalization for '{}' took {:?}",
                                current_dir, canon_elapsed
                            );
                        }

                        let path_key = if let Ok(canon) = canon_res {
                            canon.to_string_lossy().to_string()
                        } else {
                            current_dir.clone()
                        };

                        if !visited.insert(path_key.clone()) {
                            if is_logged {
                                println!("[SEARCH THREAD] Skipping already visited directory: '{}' (canonical: '{}')", current_dir, path_key);
                            }
                            continue;
                        }

                        visited_count += 1;
                        if is_logged {
                            println!(
                                "[SEARCH THREAD] Visited {} directories. Processing current: '{}'",
                                visited_count, current_dir
                            );
                            let send_scan_start = std::time::Instant::now();
                            let _ = tx_bg.send(SearchMessage::ScanningDir(current_dir.clone()));
                            println!(
                                "[SEARCH THREAD] Sending ScanningDir for '{}' took {:?}",
                                current_dir,
                                send_scan_start.elapsed()
                            );
                        }

                        let read_dir_start = std::time::Instant::now();
                        let read_dir_res = std::fs::read_dir(&current_dir);
                        let read_dir_elapsed = read_dir_start.elapsed();
                        if is_logged || read_dir_elapsed.as_millis() > 5 {
                            println!(
                                "[SEARCH THREAD] read_dir for '{}' took {:?}",
                                current_dir, read_dir_elapsed
                            );
                        }

                        let read_dir = match read_dir_res {
                            Ok(rd) => rd,
                            Err(e) => {
                                println!("[SEARCH THREAD] Permission denied or error reading directory '{}': {}", current_dir, e);
                                continue;
                            }
                        };

                        let mut entry_idx = 0;
                        for entry_res in read_dir {
                            entry_idx += 1;
                            if atomic_cancel_bg.load(std::sync::atomic::Ordering::Relaxed) {
                                println!("[SEARCH THREAD] Stop/cancel signal detected inside directory entry loop.");
                                break;
                            }

                            let entry_start = std::time::Instant::now();
                            let entry = match entry_res {
                                Ok(e) => e,
                                Err(err) => {
                                    println!(
                                        "[SEARCH THREAD] Error reading entry #{} in '{}': {}",
                                        entry_idx, current_dir, err
                                    );
                                    continue;
                                }
                            };
                            let entry_elapsed = entry_start.elapsed();
                            if entry_elapsed.as_millis() > 5 {
                                println!(
                                    "[SEARCH THREAD] Fetching entry #{} in '{}' took {:?}",
                                    entry_idx, current_dir, entry_elapsed
                                );
                            }

                            let name = entry.file_name().to_string_lossy().to_string();
                            if skip_hidden && name.starts_with('.') {
                                continue;
                            }

                            let metadata_start = std::time::Instant::now();
                            let metadata_res = entry.metadata();
                            let metadata_elapsed = metadata_start.elapsed();
                            if metadata_elapsed.as_millis() > 5 {
                                println!(
                                    "[SEARCH THREAD] Metadata syscall for '{}' took {:?}",
                                    name, metadata_elapsed
                                );
                            }

                            if let Ok(metadata) = metadata_res {
                                let full_path = join_paths(&current_dir, &name);
                                if metadata.is_dir() {
                                    let sym_start = std::time::Instant::now();
                                    let sym_res = std::fs::symlink_metadata(&full_path);
                                    let sym_elapsed = sym_start.elapsed();
                                    if sym_elapsed.as_millis() > 5 {
                                        println!("[SEARCH THREAD] Symlink metadata syscall for '{}' took {:?}", full_path, sym_elapsed);
                                    }

                                    if let Ok(sym_meta) = sym_res {
                                        if sym_meta.file_type().is_symlink() {
                                            println!(
                                                "[SEARCH THREAD] Skipping symlink directory: '{}'",
                                                full_path
                                            );
                                            continue;
                                        }
                                    }
                                    if is_logged {
                                        println!("[SEARCH THREAD] Pushing directory to queue: '{}'. New queue size: {}", full_path, queue.len() + 1);
                                    }
                                    queue.push(full_path);
                                } else {
                                    if name.to_lowercase().contains(&query_lower) {
                                        found_count += 1;
                                        println!(
                                            "[SEARCH THREAD] Found match #{}: '{}' in '{}'",
                                            found_count, name, current_dir
                                        );

                                        let mut date_str = "N/A".to_string();
                                        if let Ok(modified) = metadata.modified() {
                                            let datetime: chrono::DateTime<chrono::Local> =
                                                modified.into();
                                            date_str =
                                                datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                                        }

                                        let res = SearchResult {
                                            name: name.clone(),
                                            size: metadata.len(),
                                            date: date_str,
                                            folder: current_dir.clone(),
                                        };

                                        println!(
                                            "[SEARCH THREAD] Sending match #{} to UI receiver...",
                                            found_count
                                        );
                                        let send_start = std::time::Instant::now();
                                        let send_res = tx_bg.send(SearchMessage::Match(res));
                                        let send_elapsed = send_start.elapsed();
                                        println!(
                                            "[SEARCH THREAD] Sending match #{} took {:?}",
                                            found_count, send_elapsed
                                        );
                                        if send_res.is_err() {
                                            println!("[SEARCH THREAD] Receiver dropped. Aborting thread execution.");
                                            return;
                                        }

                                        if found_count >= 1000 {
                                            println!("[SEARCH THREAD] Reached result cap (1000 matches). Exiting.");
                                            let _ = tx_bg.send(SearchMessage::Finished(true));
                                            return;
                                        }
                                    }
                                }
                            } else {
                                if is_logged {
                                    println!("[SEARCH THREAD] Failed to retrieve metadata for '{}' in '{}'", name, current_dir);
                                }
                            }
                        }
                    }
                    println!("[SEARCH THREAD] Search traversal completed. Visited {} directories. Found {} matches.", visited_count, found_count);
                    let _ = tx_bg.send(SearchMessage::Finished(false));
                });
            } else {
                let tx_remote = tx.clone();
                let _cancel_flag_remote = cancel_flag_inner.clone();
                let _destroyed_remote = destroyed_inner.clone();
                let start_dir_remote = start_dir.clone();
                let query_remote = query.clone();
                let provider_remote = provider.clone();

                let remote_factory = crate::transfer_plan::EndpointPlan::of(&provider_remote)
                    .map(|plan| plan.into_factory());
                let atomic_cancel_remote = atomic_cancel.clone();
                let walk = move |provider_worker: Rc<dyn FileSystemRpc>| async move {
                    let mut queue = vec![start_dir_remote];
                    let query_lower = query_remote.to_lowercase();
                    let mut found_count = 0;
                    let mut visited = std::collections::HashSet::new();
                    let mut visited_count = 0;
                    let mut limit_reached = false;

                    while !queue.is_empty() {
                        if atomic_cancel_remote.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }

                        let current_dir = queue.remove(0);
                        println!("[SEARCH REMOTE] Popping directory from queue: '{}'. Remaining queue size: {}", current_dir, queue.len());

                        let path_key = current_dir.clone();
                        if !visited.insert(path_key.clone()) {
                            println!(
                                "[SEARCH REMOTE] Skipping already visited remote directory: '{}'",
                                current_dir
                            );
                            continue;
                        }

                        visited_count += 1;
                        if visited_count % 10 == 0 {
                            println!(
                                "[SEARCH REMOTE] Visited {} remote directories. Current: '{}'",
                                visited_count, current_dir
                            );
                            let _ = tx_remote.send(SearchMessage::ScanningDir(current_dir.clone()));
                        }

                        if atomic_cancel_remote.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }

                        println!(
                            "[SEARCH REMOTE] Requesting list_dir_async for '{}'...",
                            current_dir
                        );
                        let list_start = std::time::Instant::now();
                        let res =
                            list_dir_async(provider_worker.clone(), current_dir.clone()).await;
                        println!(
                            "[SEARCH REMOTE] list_dir_async for '{}' finished in {:?}",
                            current_dir,
                            list_start.elapsed()
                        );

                        if atomic_cancel_remote.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }

                        match res {
                            Ok((dirs, files)) => {
                                println!("[SEARCH REMOTE] Successfully listed '{}'. Found {} subdirs and {} files.", current_dir, dirs.len(), files.len());
                                for d in dirs {
                                    if skip_hidden && d.starts_with('.') {
                                        continue;
                                    }
                                    let full_dir_path = join_paths(&current_dir, &d);
                                    println!(
                                        "[SEARCH REMOTE] Pushing remote directory to queue: '{}'",
                                        full_dir_path
                                    );
                                    queue.push(full_dir_path);
                                }

                                for (f_name, f_size) in files {
                                    if skip_hidden && f_name.starts_with('.') {
                                        continue;
                                    }
                                    if atomic_cancel_remote.load(std::sync::atomic::Ordering::Relaxed) {
                                        break;
                                    }

                                    if f_name.to_lowercase().contains(&query_lower) {
                                        found_count += 1;
                                        println!(
                                            "[SEARCH REMOTE] Query matched: '{}' in '{}'",
                                            f_name, current_dir
                                        );

                                        let res = SearchResult {
                                            name: f_name.clone(),
                                            size: f_size,
                                            date: "N/A".to_string(),
                                            folder: current_dir.clone(),
                                        };

                                        println!("[SEARCH REMOTE] Sending remote match #{} to UI receiver...", found_count);
                                        let send_start = std::time::Instant::now();
                                        let send_res = tx_remote.send(SearchMessage::Match(res));
                                        println!(
                                            "[SEARCH REMOTE] Send match #{} took {:?}",
                                            found_count,
                                            send_start.elapsed()
                                        );
                                        if send_res.is_err() {
                                            println!("[SEARCH REMOTE] Receiver dropped. Aborting remote search.");
                                            return;
                                        }

                                        if found_count >= 1000 {
                                            println!("[SEARCH REMOTE] Reached result cap (1000 matches). Exiting.");
                                            limit_reached = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                println!(
                                    "[SEARCH REMOTE ERROR] Failed to list {}: {}",
                                    current_dir, e
                                );
                            }
                        }

                        if limit_reached {
                            break;
                        }
                    }
                    println!("[SEARCH REMOTE] Traversal finished. Sending Finished message...");
                    let _ = tx_remote.send(SearchMessage::Finished(limit_reached));
                };

                match remote_factory {
                    Some(factory) => {
                        std::thread::spawn(move || {
                            let rt = match tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                Ok(rt) => rt,
                                Err(_) => return,
                            };
                            let local = tokio::task::LocalSet::new();
                            local.block_on(&rt, async move {
                                walk(factory()).await;
                            });
                        });
                    }
                    None => {
                        glib::spawn_future_local(async move {
                            walk(provider_remote.clone()).await;
                        });
                    }
                }
            }

            let searching = searching_inner.clone();
            let cancel_flag = cancel_flag_inner.clone();
            let destroyed = destroyed_inner.clone();
            let list_box = list_box_inner.clone();
            let result_paths = paths_inner.clone();
            let status_lbl = status_lbl_inner.clone();
            let fm_menu = active_fm_search_clone.clone();
            let dialog_clone = dialog_for_search.clone();
            let btn = btn_inner.clone();

            let mut pending_matches = Vec::new();
            let mut received_count = 0;

            glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
                if *destroyed.borrow() {
                    return glib::ControlFlow::Break;
                }

                let mut channel_disconnected = false;

                loop {
                    match rx.try_recv() {
                        Ok(msg) => {
                            match msg {
                                SearchMessage::ScanningDir(dir) => {
                                    println!("[SEARCH UI] Scanning directory update: '{}'", dir);
                                    status_lbl.set_text(&*crate::i18n::trf("find.status_searching", &[("dir", &*(dir).to_string())]));
                                }
                                SearchMessage::Match(res) => {
                                    received_count += 1;
                                    pending_matches.push(res);
                                }
                                SearchMessage::Finished(capped) => {
                                    println!("[SEARCH UI] Traversal finished message received. capped: {}", capped);
                                    *searching.borrow_mut() = false;

                                    btn.set_sensitive(true);
                                    btn.set_label(&*crate::i18n::tr("find.search_btn"));
                                    btn.add_css_class("suggested-action");
                                    btn.remove_css_class("destructive-action");

                                    if capped {
                                        status_lbl.set_text(
                                            &*crate::i18n::tr("find.status_completed_capped"),
                                        );
                                    } else {
                                        status_lbl.set_text(&*crate::i18n::trf("find.status_completed", &[("count", &*(received_count.to_string()).to_string())]));
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                            break;
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                            channel_disconnected = true;
                            break;
                        }
                    }
                }

                if channel_disconnected && *searching.borrow() {
                    println!("[SEARCH UI] Channel disconnected unexpectedly.");
                    *searching.borrow_mut() = false;
                    btn.set_sensitive(true);
                    btn.set_label(&*crate::i18n::tr("find.search_btn"));
                    btn.add_css_class("suggested-action");
                    btn.remove_css_class("destructive-action");
                    status_lbl.set_text(&*crate::i18n::trf("find.status_completed", &[("count", &*(received_count.to_string()).to_string())]));
                }

                if *cancel_flag.borrow() && *searching.borrow() {
                    *searching.borrow_mut() = false;
                    btn.set_sensitive(true);
                    btn.set_label(&*crate::i18n::tr("find.search_btn"));
                    btn.add_css_class("suggested-action");
                    btn.remove_css_class("destructive-action");
                    status_lbl
                        .set_text(&*crate::i18n::trf("find.status_stopped", &[("count", &*(received_count.to_string()).to_string())]));
                }

                if !pending_matches.is_empty() {
                    let limit = std::cmp::min(pending_matches.len(), 20);
                    let matches_to_add: Vec<_> = pending_matches.drain(0..limit).collect();
                    for res in matches_to_add {
                        let size_str = format_size(res.size);
                        let row_box = Box::builder()
                            .orientation(Orientation::Horizontal)
                            .spacing(12)
                            .margin_start(8)
                            .margin_end(8)
                            .margin_top(4)
                            .margin_bottom(4)
                            .build();

                        let lbl_name = Label::builder()
                            .label(&res.name)
                            .halign(Align::Start)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .build();
                        let lbl_size = Label::builder()
                            .label(&size_str)
                            .halign(Align::Start)
                            .build();
                        let lbl_date = Label::builder()
                            .label(&res.date)
                            .halign(Align::Start)
                            .build();
                        let lbl_folder = Label::builder()
                            .label(&res.folder)
                            .halign(Align::Start)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .hexpand(true)
                            .build();

                        lbl_name.set_width_request(180);
                        lbl_size.set_width_request(80);
                        lbl_date.set_width_request(140);

                        row_box.append(&lbl_name);
                        row_box.append(&lbl_size);
                        row_box.append(&lbl_date);
                        row_box.append(&lbl_folder);

                        let row = ListBoxRow::new();
                        row.set_child(Some(&row_box));

                        let gesture = gtk::GestureClick::builder().button(3).build();

                        let fm_menu_inner = fm_menu.clone();
                        let dialog_menu = dialog_clone.clone();
                        let res_name = res.name.clone();
                        let res_folder = res.folder.clone();
                        let list_box_weak = list_box.downgrade();
                        let row_weak = row.downgrade();
                        let provider_for_gesture = provider.clone();

                        gesture.connect_pressed(move |g, _, x, y| {
                            let popover = gtk::Popover::builder()
                                .position(gtk::PositionType::Bottom)
                                .has_arrow(true)
                                .build();

                            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
                            vbox.set_margin_start(4);
                            vbox.set_margin_end(4);
                            vbox.set_margin_top(4);
                            vbox.set_margin_bottom(4);

                            let create_btn = |label: &str, icon: &str, css: &str| -> gtk::Button {
                                let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                                let img = gtk::Image::from_resource(icon);
                                img.set_pixel_size(16);
                                b.append(&img);
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

                            let btn_go = create_btn(&*crate::i18n::tr("find.open_location"), "/com/icecommander/gtk/open.svg", "");
                            let pop_go = popover.clone();
                            let folder_go = res_folder.clone();
                            let fm_go = fm_menu_inner.clone();
                            let dialog_go = dialog_menu.clone();
                            btn_go.connect_clicked(move |_| {
                                pop_go.popdown();
                                println!("[SEARCH UI] Navigating to: '{}'", folder_go);
                                fm_go.open_path(folder_go.clone());
                                dialog_go.close();
                            });
                            vbox.append(&btn_go);

                            let btn_copy = create_btn(&*crate::i18n::tr("find.copy_path"), "/com/icecommander/gtk/copy.svg", "");
                            let pop_copy = popover.clone();
                            let full_path = join_paths(&res_folder, &res_name);
                            btn_copy.connect_clicked(move |_| {
                                pop_copy.popdown();
                                if let Some(display) = gtk::gdk::Display::default() {
                                    display.clipboard().set_text(&full_path);
                                }
                            });
                            vbox.append(&btn_copy);

                            let sep = gtk::Separator::builder()
                                .orientation(gtk::Orientation::Horizontal)
                                .build();
                            vbox.append(&sep);

                            let btn_del =
                                create_btn(&*crate::i18n::tr("find.delete"), "/com/icecommander/gtk/delete-file.svg", "destructive-action");
                            let pop_del = popover.clone();
                            let full_path_del = join_paths(&res_folder, &res_name);
                            let provider_del = provider_for_gesture.clone();
                            let dialog_del = dialog_menu.clone();
                            let res_name_del = res_name.clone();
                            let lb_weak = list_box_weak.clone();
                            let rw_weak = row_weak.clone();

                            btn_del.connect_clicked(move |_| {
                                pop_del.popdown();

                                let confirm_dialog = adw::AlertDialog::builder()
                                    .heading(&*crate::i18n::tr("find.confirm_delete_title"))
                                    .body(&*crate::i18n::trf("find.confirm_delete_body", &[("name", &*(res_name_del).to_string())]))
                                    .build();

                                confirm_dialog.add_response("cancel", &*crate::i18n::tr("find.cancel"));
                                confirm_dialog.add_response("delete", &*crate::i18n::tr("find.delete"));
                                confirm_dialog.set_response_appearance(
                                    "delete",
                                    adw::ResponseAppearance::Destructive,
                                );

                                let provider_d2 = provider_del.clone();
                                let path_d2 = full_path_del.clone();
                                let lb_inner = lb_weak.clone();
                                let rw_inner = rw_weak.clone();

                                confirm_dialog.connect_response(None, move |d, response| {
                                    if response == "delete" {
                                        println!("[SEARCH UI] Deleting: '{}'", path_d2);
                                        let provider_d3 = provider_d2.clone();
                                        let path_d3 = path_d2.clone();
                                        let lb_del = lb_inner.clone();
                                        let rw_del = rw_inner.clone();
                                        gtk::glib::spawn_future_local(async move {
                                            provider_d3.delete_entries(vec![path_d3]).await.ok();
                                            if let Some(list_box_widget) = lb_del.upgrade() {
                                                if let Some(row_widget) = rw_del.upgrade() {
                                                    list_box_widget.remove(&row_widget);
                                                }
                                            }
                                        });
                                    }
                                    d.close();
                                });
                                confirm_dialog.present(Some(&dialog_del));
                            });
                            vbox.append(&btn_del);

                            popover.set_child(Some(&vbox));

                            if let Some(widget) = g.widget() {
                                popover.set_parent(&widget);
                                g.set_state(gtk::EventSequenceState::Claimed);
                                let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                                popover.set_pointing_to(Some(&rect));
                                popover.popup();
                            }
                        });
                        row.add_controller(gesture);

                        list_box.append(&row);
                        result_paths.borrow_mut().push(res.folder);
                    }
                }

                let is_searching = *searching.borrow();
                let has_pending = !pending_matches.is_empty();

                if !is_searching && !has_pending {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }
    };

    let trigger_search_c = trigger_search.clone();
    search_btn.connect_clicked(move |_| {
        trigger_search_c();
    });

    let trigger_search_c2 = trigger_search.clone();
    search_entry.connect_activate(move |_| {
        trigger_search_c2();
    });

    dialog.present();
    search_entry.grab_focus();
}
