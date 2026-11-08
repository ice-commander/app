use adw::prelude::*;

use crate::transfer_plan::EndpointPlan;
use fm_core::rpc::FileSystemRpc;
use gtk::glib;
use transfer_core::{
    execute_transfer, execute_transfer_offthread, find_clashes, format_size, into_itself,
    leave_out_items_onto_themselves, mode_of, move_by_rename, scan_items, skip_clashes, Clash,
    ClashKind, CopyMessage, ErrorAction, ErrorRequest, TransferItem, Unchecked,
};

use common::AppError;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct TransferSource {
    pub items: Vec<(String, bool, u64, Option<u32>)>,
    pub parent: String,
    pub provider: std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>,
    pub refresh: Option<Rc<panel_router::PanelRouter>>,
    pub on_done: Option<Rc<dyn Fn(&[String])>>,
    pub dest_names: std::collections::HashMap<String, String>,
}

impl TransferSource {
    pub fn from_panel(fm: &Rc<panel_router::PanelRouter>) -> Self {
        Self {
            items: get_selected_items_info(fm),
            parent: fm.current_path_string(),
            provider: fm.provider(),
            refresh: Some(fm.clone()),
            on_done: None,
            dest_names: std::collections::HashMap::new(),
        }
    }
}

pub fn trigger_copy(
    window: &adw::ApplicationWindow,
    active_fm: &Rc<panel_router::PanelRouter>,
    inactive_fm: &Rc<panel_router::PanelRouter>,
) {
    show_transfer_dialog(
        window,
        TransferSource::from_panel(active_fm),
        inactive_fm,
        false,
        None,
    );
}

pub fn trigger_move(
    window: &adw::ApplicationWindow,
    active_fm: &Rc<panel_router::PanelRouter>,
    inactive_fm: &Rc<panel_router::PanelRouter>,
) {
    show_transfer_dialog(
        window,
        TransferSource::from_panel(active_fm),
        inactive_fm,
        true,
        None,
    );
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

fn get_selected_items_info(
    fm: &Rc<panel_router::PanelRouter>,
) -> Vec<(String, bool, u64, Option<u32>)> {
    fm.selected_entries()
        .into_iter()
        .filter(|entry| entry.name() != "..")
        .map(|entry| {
            (
                entry.name(),
                entry.is_dir(),
                entry.size(),
                entry.permissions(),
            )
        })
        .collect()
}

pub fn show_transfer_dialog(
    window: &adw::ApplicationWindow,
    source: TransferSource,
    inactive_fm: &Rc<panel_router::PanelRouter>,
    is_move: bool,
    dest_override: Option<String>,
) {
    let selected_items = source.items.clone();

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

    let src_parent = source.parent.clone();
    let dest_parent = dest_override.unwrap_or_else(|| inactive_fm.current_path_string());

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
    dialog.add_response(
        action_name,
        &*crate::i18n::tr(if is_move {
            "fm.action_move"
        } else {
            "fm.action_copy"
        }),
    );
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

    let src_provider = source.provider.clone();

    let scan_future = scan_items(
        src_provider,
        selected_items.clone(),
        src_parent.clone(),
        move |files, dirs, bytes| {
            let size_str = format_size(bytes);
            scan_label_c.set_text(&crate::i18n::trf(
                "fm.scale_status",
                &[
                    ("files", &*(files.to_string()).to_string()),
                    ("dirs", &*(dirs.to_string()).to_string()),
                    ("size", &*(size_str).to_string()),
                ],
            ));
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

    let source_c = std::rc::Rc::new(source);
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
                    source_c.clone(),
                    inactive_fm_c.clone(),
                    items,
                    dest_path_str,
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

struct Side {
    provider: Rc<dyn FileSystemRpc>,
    plan: Option<EndpointPlan>,
}

enum Prepared {
    IntoItself { folder: String, into: String },
    Ready(Ready),
}

struct Ready {
    items: Vec<TransferItem>,
    clashes: Vec<Clash>,
    unchecked: Option<String>,
    dest: Side,
    dest_parent: String,
    offthread: bool,
}

async fn on_a_worker<T, F>(work: impl FnOnce() -> F + Send + 'static) -> Result<T, AppError>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, AppError>>,
{
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AppError::Other(format!("worker runtime: {e}")))?;
        tokio::task::LocalSet::new().block_on(&rt, work())
    })
    .await
    .map_err(|e| AppError::Other(format!("worker task: {e}")))?
}

async fn check_into_itself(
    src: &Side,
    dest: &Side,
    items: &[TransferItem],
    dest_parent: &str,
    offthread: &mut bool,
) -> Option<(String, String)> {
    if let (Some(src_plan), Some(dest_plan)) = (&src.plan, &dest.plan) {
        let src_factory = src_plan.clone().into_factory();
        let dest_factory = dest_plan.clone().into_factory();
        let (sent, at) = (items.to_vec(), dest_parent.to_string());
        let asked = on_a_worker(move || async move {
            Ok(into_itself(&src_factory(), &dest_factory(), &sent, &at).await)
        })
        .await;
        match asked {
            Ok(refused) => return refused,
            Err(why) => {
                ic_logging::warn!("into-itself check off the main thread failed: {why}");
                *offthread = false;
            }
        }
    }
    into_itself(&src.provider, &dest.provider, items, dest_parent).await
}

fn standing_in_it(dest: &Rc<dyn FileSystemRpc>, dest_parent: &str) -> bool {
    dest.plugin_scope().is_some()
        && dest
            .as_any()
            .and_then(|any| any.downcast_ref::<panel_router::RoutingProvider>())
            .is_some_and(|routed| routed.resolve(dest_parent) == "/")
}

async fn check_destination(
    src: &Side,
    dest: &Side,
    items: &mut Vec<TransferItem>,
    dest_parent: &str,
    cancellation_flag: &Arc<AtomicBool>,
    offthread: &mut bool,
) -> Result<Vec<Clash>, Unchecked> {
    if let (Some(dest_plan), true) = (&dest.plan, *offthread) {
        let src_factory = src.plan.clone().map(EndpointPlan::into_factory);
        let mut sent = items.clone();
        if src_factory.is_none() {
            leave_out_items_onto_themselves(&src.provider, &dest.provider, dest_parent, &mut sent);
        }
        let dest_factory = dest_plan.clone().into_factory();
        let (at, flag) = (dest_parent.to_string(), cancellation_flag.clone());
        let checked = on_a_worker(move || async move {
            let dest = dest_factory();
            if let Some(src_factory) = src_factory {
                leave_out_items_onto_themselves(&src_factory(), &dest, &at, &mut sent);
            }
            let clashes = find_clashes(dest, &sent, &at, &flag).await;
            Ok((sent, clashes))
        })
        .await;
        let why = match checked {
            Ok((_, Err(why))) if why != Unchecked::Failed(AppError::Cancelled) => {
                format!("{why:?}")
            }
            Ok((sent, clashes)) => {
                *items = sent;
                return clashes;
            }
            Err(why) => why.to_string(),
        };
        if cancellation_flag.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled.into());
        }
        ic_logging::warn!("destination check off the main thread failed: {why}");
        *offthread = false;
    }
    leave_out_items_onto_themselves(&src.provider, &dest.provider, dest_parent, items);
    find_clashes(dest.provider.clone(), items, dest_parent, cancellation_flag).await
}

async fn prepare(
    src: &Side,
    dest: &Side,
    mut items: Vec<TransferItem>,
    dest_parent: &str,
    cancellation_flag: &Arc<AtomicBool>,
) -> Result<Prepared, AppError> {
    let mut offthread = true;
    if let Some((folder, into)) =
        check_into_itself(src, dest, &items, dest_parent, &mut offthread).await
    {
        return Ok(Prepared::IntoItself { folder, into });
    }
    if cancellation_flag.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    // A destination a plugin claims is that file's inside, made if absent: packing.
    let packing = if standing_in_it(&dest.provider, dest_parent) {
        None
    } else {
        fm_core::plugin_fs::destination_inside(&dest.provider, dest_parent).await
    };
    let (dest, dest_parent) = match packing {
        Some(Ok(inside)) => (
            Side {
                provider: inside,
                plan: None,
            },
            "/".to_string(),
        ),
        Some(Err(why)) => return Err(why),
        None => (
            Side {
                provider: dest.provider.clone(),
                plan: dest.plan.clone(),
            },
            dest_parent.to_string(),
        ),
    };
    let checked = check_destination(
        src,
        &dest,
        &mut items,
        &dest_parent,
        cancellation_flag,
        &mut offthread,
    )
    .await;
    let (clashes, unchecked) = match checked {
        Ok(clashes) => (clashes, None),
        Err(Unchecked::Unreadable { folder, .. }) => (Vec::new(), Some(folder)),
        Err(Unchecked::Failed(why)) => return Err(why),
    };
    Ok(Prepared::Ready(Ready {
        items,
        clashes,
        unchecked,
        dest,
        dest_parent,
        offthread,
    }))
}

async fn while_checking<T>(
    window: &adw::ApplicationWindow,
    cancellation_flag: &Arc<AtomicBool>,
    work: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let mut work = std::pin::pin!(work);
    tokio::select! {
        done = &mut work => return done,
        () = glib::timeout_future(std::time::Duration::from_millis(250)) => {}
    }

    let spinner = gtk::Spinner::builder().spinning(true).build();
    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("fm.checking_destination"))
        .extra_child(&spinner)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    let cancellation_flag_c = cancellation_flag.clone();
    dialog.connect_response(None, move |_d, _response| {
        cancellation_flag_c.store(true, Ordering::Relaxed);
    });
    dialog.present(Some(window));

    let done = work.await;
    if cancellation_flag.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    dialog.close();
    done
}

async fn move_by_rename_offthread(
    items: Vec<TransferItem>,
    dest_parent: String,
) -> Result<Vec<TransferItem>, AppError> {
    tokio::task::spawn_blocking(move || {
        let mut items = items;
        move_by_rename(&mut items, &dest_parent);
        items
    })
    .await
    .map_err(|e| AppError::Other(format!("rename task: {e}")))
}

async fn resolve_conflicts(
    window: &adw::ApplicationWindow,
    mut items: Vec<TransferItem>,
    clashes: &[Clash],
) -> Option<Vec<TransferItem>> {
    let mut apply_all: Option<bool> = match clashes.len() {
        0 => return Some(items),
        1 => None,
        _ => ask_overwrite_all(window, clashes).await?.then_some(true),
    };
    let summarized = clashes.len() > 1;
    let replaceable = |clash: &Clash| matches!(clash.kind, ClashKind::ReplacesFile { .. });

    let mut skipped: Vec<Clash> = Vec::new();
    for (idx, clash) in clashes.iter().enumerate() {
        let overwrite = match apply_all {
            Some(v) => v,
            None if summarized && !replaceable(clash) => false,
            None => {
                let remaining = clashes[idx..]
                    .iter()
                    .filter(|other| replaceable(other))
                    .count();
                match ask_overwrite_each(window, clash, remaining).await {
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
                }
            }
        };
        if !overwrite || !replaceable(clash) {
            skipped.push(clash.clone());
        }
    }

    skip_clashes(&mut items, &skipped);
    Some(items)
}

async fn ask_overwrite_all(window: &adw::ApplicationWindow, clashes: &[Clash]) -> Option<bool> {
    let mut replaced = 0usize;
    let mut replaced_bytes = 0u64;
    let mut inside = 0usize;
    for clash in clashes {
        if let ClashKind::ReplacesFile { size } = clash.kind {
            replaced += 1;
            replaced_bytes += size;
            if std::path::Path::new(&clash.relative_path)
                .components()
                .nth(1)
                .is_some()
            {
                inside += 1;
            }
        }
    }
    let mismatched = clashes.len() - replaced;

    let mut lines = Vec::new();
    if replaced > 0 {
        lines.push(crate::i18n::trf(
            "fm.clash_summary_replace",
            &[
                ("count", &replaced.to_string()),
                ("size", &format_size(replaced_bytes)),
            ],
        ));
    }
    if inside > 0 {
        lines.push(crate::i18n::trf(
            "fm.clash_summary_inside",
            &[("count", &inside.to_string())],
        ));
    }
    if mismatched > 0 {
        lines.push(crate::i18n::trf(
            "fm.clash_summary_mismatch",
            &[("count", &mismatched.to_string())],
        ));
    }
    let mut body = lines.join("\n");
    if replaced > 0 {
        body.push_str("\n\n");
        body.push_str(&crate::i18n::tr("fm.clash_summary_ask_each"));
    }

    let check = (replaced > 0).then(|| {
        gtk::CheckButton::builder()
            .label(&*crate::i18n::tr("fm.clash_overwrite_all"))
            .halign(gtk::Align::Center)
            .build()
    });
    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("fm.clash_summary_title"))
        .body(body)
        .build();
    dialog.set_extra_child(check.as_ref());
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("continue", &crate::i18n::tr("fm.clash_continue"));
    dialog.set_default_response(Some("continue"));
    dialog.set_response_appearance("continue", adw::ResponseAppearance::Suggested);

    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Rc::new(std::cell::RefCell::new(Some(tx)));
    dialog.connect_response(None, move |d, resp| {
        let overwrite_all = check.as_ref().is_none_or(CheckButtonExt::is_active);
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send((resp == "continue").then_some(overwrite_all));
        }
        d.close();
    });
    dialog.present(Some(window));

    rx.await.ok().flatten()
}

async fn ask_overwrite_each(
    window: &adw::ApplicationWindow,
    clash: &Clash,
    remaining: usize,
) -> OverwriteDecision {
    let path = &clash.relative_path;
    let (heading, body) = match clash.kind {
        ClashKind::ReplacesFile { .. } => (
            "fm.clash_exists_title",
            crate::i18n::trf("fm.clash_exists_body", &[("path", path)]),
        ),
        ClashKind::FileOverFolder => (
            "fm.clash_mismatch_title",
            crate::i18n::trf("fm.clash_file_over_folder", &[("path", path)]),
        ),
        ClashKind::FolderOverFile => (
            "fm.clash_mismatch_title",
            crate::i18n::trf("fm.clash_folder_over_file", &[("path", path)]),
        ),
    };
    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr(heading))
        .body(body)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("skip", &crate::i18n::tr("fm.clash_skip"));
    if remaining > 1 {
        dialog.add_response("skip_all", &crate::i18n::tr("fm.clash_skip_all"));
    }
    if matches!(clash.kind, ClashKind::ReplacesFile { .. }) {
        if remaining > 1 {
            dialog.add_response("yes_all", &crate::i18n::tr("fm.clash_yes_all"));
        }
        dialog.add_response("yes", &crate::i18n::tr("fm.clash_yes"));
        dialog.set_default_response(Some("yes"));
        dialog.set_response_appearance("yes", adw::ResponseAppearance::Suggested);
    } else {
        dialog.set_default_response(Some("skip"));
    }

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

async fn ask_copy_unchecked(window: &adw::ApplicationWindow, folder: &str) -> bool {
    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("fm.clash_unchecked_title"))
        .body(crate::i18n::trf(
            "fm.clash_unchecked_body",
            &[("folder", folder)],
        ))
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("copy", &crate::i18n::tr("fm.action_copy"));
    dialog.set_default_response(Some("cancel"));

    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Rc::new(std::cell::RefCell::new(Some(tx)));
    dialog.connect_response(None, move |d, resp| {
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send(resp == "copy");
        }
        d.close();
    });
    dialog.present(Some(window));

    rx.await.unwrap_or(false)
}

fn top_level_names(items: &[TransferItem]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.relative_path == item.name)
        .map(|item| item.name.clone())
        .collect()
}

fn run_transfer_with_progress(
    window: &adw::ApplicationWindow,
    src: Side,
    dest: Side,
    items: Vec<TransferItem>,
    dest_parent: String,
    is_move: bool,
    on_finish: impl Fn(&[String]) + 'static,
) {
    let window = window.clone();

    gtk::glib::spawn_future_local(async move {
        let checking_flag = Arc::new(AtomicBool::new(false));
        let prepared = while_checking(
            &window,
            &checking_flag,
            prepare(&src, &dest, items, &dest_parent, &checking_flag),
        )
        .await;
        let Ready {
            items,
            clashes,
            unchecked,
            dest,
            dest_parent,
            offthread,
        } = match prepared {
            Ok(Prepared::Ready(ready)) => ready,
            Ok(Prepared::IntoItself { folder, into }) => {
                show_error_dialog(
                    &window,
                    &crate::i18n::tr("fm.transfer_error"),
                    &crate::i18n::trf("fm.into_itself", &[("folder", &folder), ("dest", &into)]),
                );
                on_finish(&[]);
                return;
            }
            Err(AppError::Cancelled) => {
                on_finish(&[]);
                return;
            }
            Err(why) => {
                show_error_dialog(
                    &window,
                    &crate::i18n::tr("fm.transfer_error"),
                    &why.to_string(),
                );
                on_finish(&[]);
                return;
            }
        };
        if let Some(folder) = unchecked {
            if !ask_copy_unchecked(&window, &folder).await {
                on_finish(&[]);
                return;
            }
        }
        let Some(mut remaining_items) = resolve_conflicts(&window, items, &clashes).await else {
            on_finish(&[]);
            return;
        };
        let mut moved = Vec::new();
        if is_move && src.provider.is_local() && dest.provider.is_local() {
            let before = top_level_names(&remaining_items);
            remaining_items =
                match move_by_rename_offthread(remaining_items, dest_parent.clone()).await {
                    Ok(left) => left,
                    Err(why) => {
                        show_error_dialog(
                            &window,
                            &crate::i18n::tr("fm.transfer_error"),
                            &why.to_string(),
                        );
                        on_finish(&[]);
                        return;
                    }
                };
            let left = top_level_names(&remaining_items);
            moved = before
                .into_iter()
                .filter(|name| !left.contains(name))
                .collect();
        }
        if remaining_items.is_empty() {
            on_finish(&moved);
            return;
        }

        let action_title = if is_move {
            crate::i18n::tr("fm.moving_files")
        } else {
            crate::i18n::tr("fm.copying_files")
        };

        let progress_dialog = adw::AlertDialog::builder().heading(action_title).build();

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

        glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
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
                            CopyMessage::Progress {
                                file_bytes,
                                total_bytes_copied,
                            } => {
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

            if state_changed {
                ic_logging::info!(
                    "copy progress: {} {}/{} bytes, overall {}/{} bytes, {}/{} files",
                    current_file_name,
                    current_file_copied,
                    current_file_size,
                    overall_copied_bytes,
                    total_bytes,
                    files_copied,
                    total_files
                );
                api_op.update(&current_file_name, overall_copied_bytes, files_copied);
                current_file_label_c.set_text(&crate::i18n::trf(
                    "fm.transferring_file",
                    &[("file", &*(current_file_name).to_string())],
                ));
                if current_file_size > 0 {
                    let frac =
                        (current_file_copied as f64 / current_file_size as f64).clamp(0.0, 1.0);
                    current_file_progress_c.set_fraction(frac);
                } else {
                    current_file_progress_c.set_fraction(0.0);
                }

                if total_bytes > 0 {
                    let overall_frac =
                        (overall_copied_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);
                    overall_progress_c.set_fraction(overall_frac);

                    let copied_size_str = format_size(overall_copied_bytes);
                    let total_size_str = format_size(total_bytes);

                    overall_label_c.set_text(&crate::i18n::trf(
                        "fm.overall_progress_stats",
                        &[
                            ("copied_size", &*(copied_size_str).to_string()),
                            ("total_size", &*(total_size_str).to_string()),
                            ("files_copied", &*(files_copied.to_string()).to_string()),
                            ("total_files", &*(total_files.to_string()).to_string()),
                        ],
                    ));
                } else {
                    let frac = if total_files > 0 {
                        (files_copied as f64 / total_files as f64).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    overall_progress_c.set_fraction(frac);
                    overall_label_c.set_text(&crate::i18n::trf(
                        "fm.overall_progress_files",
                        &[
                            ("files_copied", &*(files_copied.to_string()).to_string()),
                            ("total_files", &*(total_files.to_string()).to_string()),
                        ],
                    ));
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

        let plans = if offthread {
            src.plan.zip(dest.plan)
        } else {
            None
        };
        let sent = top_level_names(&remaining_items);
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
                    src.provider,
                    dest.provider,
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
                show_error_dialog(
                    &window,
                    &crate::i18n::tr("fm.transfer_error"),
                    &e.to_string(),
                );
            }
        }
        if is_move && res.is_ok() {
            moved.extend(sent);
        }
        on_finish(&moved);
    });
}

fn show_progress_dialog(
    window: &adw::ApplicationWindow,
    source: Rc<TransferSource>,
    inactive_fm: Rc<panel_router::PanelRouter>,
    items: Vec<TransferItem>,
    dest_parent: String,
    is_move: bool,
) {
    let src_provider = source.provider.clone();
    let dest_provider = inactive_fm.provider();
    let src_plan = EndpointPlan::of(&src_provider);
    let dest_plan = EndpointPlan::of(&dest_provider);

    let mut items = items;
    transfer_core::retarget_top_level(&mut items, &source.dest_names);

    let source_r = source.clone();
    let inactive_fm_c = inactive_fm.clone();
    run_transfer_with_progress(
        window,
        Side {
            provider: src_provider,
            plan: src_plan,
        },
        Side {
            provider: dest_provider,
            plan: dest_plan,
        },
        items,
        dest_parent,
        is_move,
        move |moved| {
            if let Some(r) = &source_r.refresh {
                r.refresh_spawned();
            }
            if let Some(f) = &source_r.on_done {
                f(moved);
            }
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
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
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

    let src_provider = std::rc::Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
        dest_fm.config(),
    ));

    let scan_future = scan_items(
        src_provider,
        selected_items.clone(),
        src_parent.clone(),
        move |files, dirs, bytes| {
            let size_str = format_size(bytes);
            scan_label_c.set_text(&crate::i18n::trf(
                "fm.scale_status",
                &[
                    ("files", &*(files.to_string()).to_string()),
                    ("dirs", &*(dirs.to_string()).to_string()),
                    ("size", &*(size_str).to_string()),
                ],
            ));
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
    let src_provider: Rc<dyn FileSystemRpc> = std::rc::Rc::new(
        localfs::local_rpc::LocalFileSystemRpc::new(dest_fm.config()),
    );
    let dest_provider = dest_fm.provider();
    let src_plan = EndpointPlan::of(&src_provider);
    let dest_plan = EndpointPlan::of(&dest_provider);

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
        Side {
            provider: src_provider,
            plan: src_plan,
        },
        Side {
            provider: dest_provider,
            plan: dest_plan,
        },
        remaining_items,
        dest_parent,
        false,
        move |_| {
            dest_fm.refresh_spawned();
            if let Some(ref sfm) = source_fm {
                sfm.refresh_spawned();
            }
        },
    );
}
