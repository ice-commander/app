use gtk::prelude::*;
use adw::prelude::*;
use gtk::cairo;
use pdfium_render::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

static PDFIUM_INSTANCE: std::sync::OnceLock<Pdfium> = std::sync::OnceLock::new();
static PDFIUM_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
static PDFIUM_CALL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pdfium_guard() -> std::sync::MutexGuard<'static, ()> {
    PDFIUM_CALL.lock().unwrap_or_else(|e| e.into_inner())
}

enum PdfEvent {
    Opened(Vec<(f32, f32)>),
    Page {
        index: u32,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Error(String),
}

enum PdfRequest {
    Render { index: u32, target_width: u32 },
}

pub(super) fn build_pdf_content(ctx: &gtk_viewer_ui::ViewerCtx, bytes: Vec<u8>) {
    super::style::ensure_loaded();
    let file_name = ctx.name.clone();
    let window = ctx.window.clone();


    let overlay = gtk::Overlay::builder()
        .hexpand(true)
        .vexpand(true)
        .build();

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();
    scrolled.add_css_class("pdf-scrolled-window");

    let pages_model = gtk::StringList::new(&[]);
    let pages_factory = gtk::SignalListItemFactory::new();
    let pages_list = gtk::ListView::new(
        Some(gtk::NoSelection::new(Some(pages_model.clone()))),
        Some(pages_factory.clone()),
    );
    pages_list.set_single_click_activate(false);
    pages_list.set_margin_start(16);
    pages_list.set_margin_end(16);
    pages_list.set_margin_top(16);
    pages_list.set_margin_bottom(16);
    scrolled.set_child(Some(&pages_list));
    overlay.set_child(Some(&scrolled));

    let zoom_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_end(24)
        .margin_top(16)
        .build();
    zoom_box.add_css_class("pdf-zoom-control");

    let btn_print = gtk::Button::builder()
        .tooltip_text("Print Document")
        .css_classes(vec!["flat".to_string()])
        .child(&super::load_icon("/com/icecommander/gtk/print.svg"))
        .margin_end(24)
        .build();
    let btn_zoom_out = gtk::Button::builder()
        .tooltip_text("Zoom Out")
        .css_classes(vec!["flat".to_string()])
        .child(&super::load_icon("/com/icecommander/gtk/zoom-out.svg"))
        .build();
    let btn_zoom_in = gtk::Button::builder()
        .tooltip_text("Zoom In")
        .css_classes(vec!["flat".to_string()])
        .child(&super::load_icon("/com/icecommander/gtk/zoom-in.svg"))
        .build();
    let zoom_label = gtk::Label::builder()
        .label("100%")
        .build();
    zoom_label.add_css_class("pdf-zoom-label");

    zoom_box.append(&btn_print);
    zoom_box.append(&btn_zoom_out);
    zoom_box.append(&zoom_label);
    zoom_box.append(&btn_zoom_in);
    overlay.add_overlay(&zoom_box);

    let status_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let spinner = gtk::Spinner::builder()
        .width_request(40)
        .height_request(40)
        .build();
    spinner.start();
    let status_label = gtk::Label::new(Some("Opening document..."));
    status_box.append(&spinner);
    status_box.append(&status_label);
    overlay.add_overlay(&status_box);

    ctx.stack.add_named(&overlay, Some("content"));
    ctx.stack.set_visible_child_name("content");

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let current_zoom = Rc::new(Cell::new(100));
    let current_width = Rc::new(Cell::new(800));
    let pdf_bytes = Rc::new(std::cell::RefCell::new(None::<Vec<u8>>));

    let page_sizes: Rc<std::cell::RefCell<Vec<(f32, f32)>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let bound: Rc<std::cell::RefCell<std::collections::HashMap<u32, gtk::Picture>>> =
        Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()));
    let pending: Rc<std::cell::RefCell<std::collections::HashSet<(u32, u32)>>> =
        Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));
    let cache: Rc<std::cell::RefCell<std::collections::VecDeque<((u32, u32), gtk::gdk::MemoryTexture)>>> =
        Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
    const CACHE_PAGES: usize = 8;

    let (tx, rx) = std::sync::mpsc::channel::<PdfEvent>();
    let (req_tx, req_rx) = std::sync::mpsc::channel::<PdfRequest>();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let status_box_clone = status_box.clone();
    let status_label_clone = status_label.clone();
    let spinner_clone = spinner.clone();
    let current_width_clone = current_width.clone();
    let page_sizes_bind = page_sizes.clone();
    let bound_bind = bound.clone();
    let pending_bind = pending.clone();
    let cache_bind = cache.clone();
    let req_tx_bind = req_tx.clone();

    pages_factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let picture = gtk::Picture::new();
        picture.set_halign(gtk::Align::Center);
        picture.add_css_class("pdf-page-picture");
        item.set_child(Some(&picture));
    });

    pages_factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(picture) = item.child().and_downcast::<gtk::Picture>() else {
            return;
        };
        let index = item.position();
        let width = current_width_clone.get();
        let ratio = page_sizes_bind
            .borrow()
            .get(index as usize)
            .map(|(w, h)| (*h / *w) as f64)
            .unwrap_or(1.414);
        picture.set_size_request(width, (width as f64 * ratio) as i32);

        let key = (index, width as u32);
        let cached = cache_bind
            .borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, t)| t.clone());
        match cached {
            Some(texture) => picture.set_paintable(Some(&texture)),
            None => {
                picture.set_paintable(None::<&gtk::gdk::Texture>);
                if pending_bind.borrow_mut().insert(key) {
                    let _ = req_tx_bind.send(PdfRequest::Render { index, target_width: width as u32 });
                }
            }
        }
        bound_bind.borrow_mut().insert(index, picture);
    });

    let bound_unbind = bound.clone();
    pages_factory.connect_unbind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let Some(picture) = item.child().and_downcast::<gtk::Picture>() {
            picture.set_paintable(None::<&gtk::gdk::Texture>);
        }
        bound_unbind.borrow_mut().remove(&item.position());
    });

    let cancelled_ui = cancelled.clone();
    let page_sizes_ui = page_sizes.clone();
    let bound_ui = bound.clone();
    let pending_ui = pending.clone();
    let cache_ui = cache.clone();
    let pages_model_ui = pages_model.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        if cancelled_ui.load(std::sync::atomic::Ordering::Relaxed) {
            return gtk::glib::ControlFlow::Break;
        }
        let mut failed = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                PdfEvent::Opened(sizes) => {
                    spinner_clone.stop();
                    status_box_clone.set_visible(false);
                    for i in 0..sizes.len() {
                        pages_model_ui.append(&(i + 1).to_string());
                    }
                    *page_sizes_ui.borrow_mut() = sizes;
                }
                PdfEvent::Page { index, width, height, pixels } => {
                    pending_ui.borrow_mut().remove(&(index, width));
                    let bytes_glib = gtk::glib::Bytes::from(&pixels);
                    let texture = gtk::gdk::MemoryTexture::new(
                        width as i32,
                        height as i32,
                        gtk::gdk::MemoryFormat::R8g8b8a8,
                        &bytes_glib,
                        (width * 4) as usize,
                    );
                    if let Some(picture) = bound_ui.borrow().get(&index) {
                        picture.set_paintable(Some(&texture));
                    }
                    let mut cache = cache_ui.borrow_mut();
                    cache.push_back(((index, width), texture));
                    while cache.len() > CACHE_PAGES {
                        cache.pop_front();
                    }
                }
                PdfEvent::Error(err) => {
                    spinner_clone.stop();
                    status_box_clone.set_visible(true);
                    status_label_clone.set_text(&format!("Error: {}", err));
                    failed = true;
                }
            }
        }
        if failed {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });

    let cancelled_close = cancelled.clone();
    window.connect_close_request(move |_| {
        cancelled_close.store(true, std::sync::atomic::Ordering::Relaxed);
        gtk::glib::Propagation::Proceed
    });

    let apply_zoom = {
        let current_zoom = current_zoom.clone();
        let current_width = current_width.clone();
        let bound_zoom = bound.clone();
        let cache_zoom = cache.clone();
        let page_sizes_zoom = page_sizes.clone();
        let pending_zoom = pending.clone();
        let req_tx_zoom = req_tx.clone();
        let zoom_label = zoom_label.clone();
        move |delta: i32| {
            let new_zoom = (current_zoom.get() + delta).clamp(40, 200);
            current_zoom.set(new_zoom);

            let new_width = (800 * new_zoom) / 100;
            current_width.set(new_width);

            zoom_label.set_text(&format!("{}%", new_zoom));

            cache_zoom.borrow_mut().clear();
            for (index, picture) in bound_zoom.borrow().iter() {
                let ratio = page_sizes_zoom
                    .borrow()
                    .get(*index as usize)
                    .map(|(w, h)| (*h / *w) as f64)
                    .unwrap_or(1.414);
                picture.set_size_request(new_width, (new_width as f64 * ratio) as i32);
                let key = (*index, new_width as u32);
                if pending_zoom.borrow_mut().insert(key) {
                    let _ = req_tx_zoom.send(PdfRequest::Render {
                        index: *index,
                        target_width: new_width as u32,
                    });
                }
            }
        }
    };

    let apply_zoom_rc = Rc::new(apply_zoom);

    let apply_zoom_in = apply_zoom_rc.clone();
    btn_zoom_in.connect_clicked(move |_| {
        apply_zoom_in(10);
    });

    let apply_zoom_out = apply_zoom_rc.clone();
    btn_zoom_out.connect_clicked(move |_| {
        apply_zoom_out(-10);
    });

    let pdf_bytes_print = pdf_bytes.clone();
    let win_print = window.clone();
    let file_name_print = file_name.clone();
    btn_print.connect_clicked(move |_| {
        let bytes_opt = pdf_bytes_print.borrow();
        if let Some(ref bytes) = *bytes_opt {
            run_print_operation(&win_print, &file_name_print, bytes.clone());
        }
    });

    let cancelled_render = cancelled.clone();
    let exe_dir_c = exe_dir.clone();
    *pdf_bytes.borrow_mut() = Some(bytes.clone());

    {
        tokio::task::spawn_blocking(move || {
            let pdfium_res = if let Some(p) = PDFIUM_INSTANCE.get() {
                Ok(p)
            } else {
                let _init = PDFIUM_INIT.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(p) = PDFIUM_INSTANCE.get() {
                    Ok(p)
                } else {
                    let lib_name = if cfg!(target_os = "macos") {
                        let bundle_libs = exe_dir_c.join("../Libs");
                        let binding_path = if bundle_libs.exists() {
                            bundle_libs.to_string_lossy().to_string()
                        } else {
                            exe_dir_c.to_string_lossy().to_string()
                        };
                        Pdfium::pdfium_platform_library_name_at_path(&binding_path)
                    } else if cfg!(target_os = "linux") {
                        if exe_dir_c.join("libpdfium.so").exists() {
                            exe_dir_c.join("libpdfium.so")
                        } else if std::path::Path::new("/usr/lib/ice-commander/libpdfium.so").exists() {
                            std::path::PathBuf::from("/usr/lib/ice-commander/libpdfium.so")
                        } else if std::path::Path::new("/usr/lib/libpdfium.so").exists() {
                            std::path::PathBuf::from("/usr/lib/libpdfium.so")
                        } else {
                            std::path::PathBuf::from("libpdfium.so")
                        }
                    } else {
                        let binding_path = exe_dir_c.to_string_lossy().to_string();
                        Pdfium::pdfium_platform_library_name_at_path(&binding_path)
                    };

                    match Pdfium::bind_to_library(&lib_name) {
                        Ok(bindings) => {
                            let pdfium = Pdfium::new(bindings);
                            let _ = PDFIUM_INSTANCE.set(pdfium);
                            Ok(PDFIUM_INSTANCE.get().unwrap())
                        }
                        Err(e) => Err(format!("Could not load PDFium library: {:?}", e)),
                    }
                }
            };

            let pdfium = match pdfium_res {
                Ok(p) => p,
                Err(err) => {
                    let _ = tx.send(PdfEvent::Error(err));
                    return;
                }
            };

            let document = match pdfium.load_pdf_from_byte_slice(&bytes, None) {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx.send(PdfEvent::Error(format!("Could not load PDF document: {:?}", e)));
                    return;
                }
            };

            let pages = document.pages();
            let sizes: Vec<(f32, f32)> = pages
                .iter()
                .map(|p| (p.width().value, p.height().value))
                .collect();
            if tx.send(PdfEvent::Opened(sizes)).is_err() {
                return;
            }

            while let Ok(PdfRequest::Render { index, target_width }) = req_rx.recv() {
                if cancelled_render.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let Ok(page) = pages.get(index as i32) else {
                    continue;
                };
                let config = PdfRenderConfig::new()
                    .set_target_width(target_width.clamp(200, 3000) as i32)
                    .set_clear_color(PdfColor::new(255, 255, 255, 255));

                let guard = pdfium_guard();
                let bitmap = match page.render_with_config(&config) {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(PdfEvent::Error(format!(
                            "Failed to render page {}: {:?}",
                            index + 1,
                            e
                        )));
                        return;
                    }
                };
                let Ok(dynamic_img) = bitmap.as_image() else {
                    continue;
                };

                drop(guard);

                let rgba_img = dynamic_img.to_rgba8();
                let width = rgba_img.width();
                let height = rgba_img.height();
                let pixels = rgba_img.into_raw();

                if tx
                    .send(PdfEvent::Page { index, width, height, pixels })
                    .is_err()
                {
                    return;
                }
            }
        });
    }
}

thread_local! {
    static ACTIVE_PRINT_DOC: std::cell::RefCell<Option<PdfDocument<'static>>> = const { std::cell::RefCell::new(None) };
}

fn run_print_operation(parent: &gtk::Window, file_name: &str, bytes: Vec<u8>) {
    let print_op = gtk::PrintOperation::new();
    print_op.set_job_name(&format!("Print - {}", file_name));

    let pdfium = match PDFIUM_INSTANCE.get() {
        Some(p) => p,
        None => {
            show_error_dialog(parent, "PDFium is not initialized.");
            return;
        }
    };

    let document = match pdfium.load_pdf_from_byte_vec(bytes, None) {
        Ok(doc) => doc,
        Err(e) => {
            show_error_dialog(parent, &format!("Could not load PDF: {:?}", e));
            return;
        }
    };

    print_op.set_n_pages(document.pages().len() as i32);

    ACTIVE_PRINT_DOC.with(|d| {
        *d.borrow_mut() = Some(document);
    });

    print_op.connect_draw_page(move |_op, print_ctx, page_nr| {
        ACTIVE_PRINT_DOC.with(|doc_cell| {
            let doc_borrow = doc_cell.borrow();
            let document = match doc_borrow.as_ref() {
                Some(d) => d,
                None => return,
            };

            let page = match document.pages().get(page_nr as PdfPageIndex) {
                Ok(p) => p,
                Err(_) => return,
            };

            let render_config = PdfRenderConfig::new()
                .set_target_width(2400)
                .set_clear_color(PdfColor::new(255, 255, 255, 255));

            let guard = pdfium_guard();
            let bitmap = match page.render_with_config(&render_config) {
                Ok(b) => b,
                Err(_) => return,
            };

            let dynamic_img = match bitmap.as_image() {
                Ok(img) => img,
                Err(_) => return,
            };
            drop(guard);

            let mut rgba_img = dynamic_img.to_rgba8();
            let width = rgba_img.width() as i32;
            let height = rgba_img.height() as i32;
            let pixels: &mut [u8] = rgba_img.as_mut();

            for chunk in pixels.chunks_exact_mut(4) {
                let r = chunk[0];
                let b = chunk[2];
                chunk[0] = b;
                chunk[2] = r;
            }

            let mut surface = match cairo::ImageSurface::create(cairo::Format::ARgb32, width, height) {
                Ok(s) => s,
                Err(_) => return,
            };

            {
                if let Ok(mut surface_data) = surface.data() {
                    surface_data[..pixels.len()].copy_from_slice(pixels);
                }
            }

            let cr = print_ctx.cairo_context();

            cr.save().unwrap();
            let print_width = print_ctx.width();
            let print_height = print_ctx.height();
            let scale_x = print_width / width as f64;
            let scale_y = print_height / height as f64;
            let scale = scale_x.min(scale_y);

            cr.scale(scale, scale);
            cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
            cr.paint().unwrap();
            cr.restore().unwrap();
        });
    });

    let parent_clone = parent.clone();
    print_op.connect_done(move |_, result| {
        ACTIVE_PRINT_DOC.with(|d| {
            *d.borrow_mut() = None;
        });

        if result == gtk::PrintOperationResult::Error {
            show_error_dialog(&parent_clone, "Print operation failed.");
        }
    });

    let _ = print_op.run(gtk::PrintOperationAction::PrintDialog, Some(parent));
}

fn show_error_dialog(parent: &gtk::Window, message: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading("Print Error")
        .body(message)
        .build();
    dialog.add_response("ok", "OK");
    dialog.present(Some(parent));
}
