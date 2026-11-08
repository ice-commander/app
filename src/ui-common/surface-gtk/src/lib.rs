use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type CloseRequest = Rc<dyn Fn() -> glib::Propagation>;

thread_local! {
    static PAGES: RefCell<Option<(adw::NavigationView, gtk::Window)>> = const { RefCell::new(None) };
    static CLOSE_REQUESTS: RefCell<Vec<(glib::WeakRef<adw::NavigationPage>, CloseRequest)>> =
        const { RefCell::new(Vec::new()) };
}

/// On a phone the main window shows what would be other windows as full-height pages pushed here.
pub fn set_page_host(pages: &adw::NavigationView, window: &gtk::Window) {
    PAGES.with(|host| *host.borrow_mut() = Some((pages.clone(), window.clone())));
}

/// Android's Back: gives the screen back if a page took all of it, else leaves the page on top, if
/// there is one, unless the page would rather stay.
pub fn go_back() -> bool {
    let Some((pages, window)) = PAGES.with(|host| host.borrow().clone()) else {
        return false;
    };
    if window.is_fullscreen() {
        window.unfullscreen();
        return true;
    }
    if pages.navigation_stack().n_items() < 2 {
        return false;
    }
    match pages.visible_page() {
        Some(page) if has_close_requests(&page) => {
            leave(&pages, &page);
            true
        }
        _ => pages.pop(),
    }
}

/// A page of the phone's main window with `content` as it is and `action` at the end of its header;
/// `None` off the phone. The content goes back to whoever lent it when the page closes.
pub fn push_page(
    title: &str,
    content: &impl IsA<gtk::Widget>,
    action: &impl IsA<gtk::Widget>,
) -> Option<adw::NavigationPage> {
    let (pages, _) = PAGES.with(|host| host.borrow().clone())?;
    let header = adw::HeaderBar::new();
    header.pack_end(action);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(content));
    let page = adw::NavigationPage::builder()
        .title(title)
        .child(&view)
        .build();
    on_popped(&pages, &page, move || {
        view.set_content(None::<&gtk::Widget>)
    });
    pages.push(&page);
    Some(page)
}

/// Leaves `page` if it is the one on top.
pub fn pop_page(page: &adw::NavigationPage) {
    PAGES.with(|host| {
        if let Some((pages, _)) = host.borrow().as_ref() {
            if pages.visible_page().as_ref() == Some(page) {
                pages.pop();
            }
        }
    });
}

fn on_popped(pages: &adw::NavigationView, page: &adw::NavigationPage, f: impl Fn() + 'static) {
    let handler = Rc::new(Cell::new(None));
    let ours = page.clone();
    let once = handler.clone();
    let id = pages.connect_popped(move |pages, popped| {
        if *popped == ours {
            f();
            if let Some(id) = once.take() {
                pages.disconnect(id);
            }
        }
    });
    handler.set(Some(id));
}

/// A page that takes the whole screen (a film) takes it from its header too.
fn hide_header_in_full_screen(
    pages: &adw::NavigationView,
    page: &adw::NavigationPage,
    content: &adw::ToolbarView,
    window: &gtk::Window,
) {
    let (weak_pages, weak_page, weak_content) =
        (pages.downgrade(), page.downgrade(), content.downgrade());
    let id = window.connect_fullscreened_notify(move |window| {
        let (Some(pages), Some(page), Some(content)) = (
            weak_pages.upgrade(),
            weak_page.upgrade(),
            weak_content.upgrade(),
        ) else {
            return;
        };
        if pages.visible_page().as_ref() == Some(&page) {
            content.set_reveal_top_bars(!window.is_fullscreen());
        }
    });
    let (window, id) = (window.downgrade(), Cell::new(Some(id)));
    on_popped(pages, page, move || {
        if let (Some(window), Some(id)) = (window.upgrade(), id.take()) {
            window.disconnect(id);
        }
    });
}

fn close_requests(page: &adw::NavigationPage) -> Vec<CloseRequest> {
    CLOSE_REQUESTS.with(|all| {
        all.borrow()
            .iter()
            .filter(|(of, _)| of.upgrade().as_ref() == Some(page))
            .map(|(_, request)| request.clone())
            .collect()
    })
}

fn has_close_requests(page: &adw::NavigationPage) -> bool {
    !close_requests(page).is_empty()
}

/// Down to the page under `page`, so whatever was opened on top of it closes too; asked first,
/// one by one like a window's close-request, and the first that keeps it open ends it.
fn leave(pages: &adw::NavigationView, page: &adw::NavigationPage) {
    for request in close_requests(page) {
        if request() == glib::Propagation::Stop {
            return;
        }
    }
    // Full screen belonged to the page; the window under it is the file manager.
    if let Some((_, window)) = PAGES.with(|host| host.borrow().clone()) {
        if window.is_fullscreen() {
            window.unfullscreen();
        }
    }
    if let Some(below) = pages.previous_page(page) {
        pages.pop_to_page(&below);
    }
}

/// A window of its own on the desktop; on a phone, where every window would be an Android activity
/// of its own, a page over the main window.
#[derive(Clone)]
pub enum Surface {
    Window(gtk::Window),
    Page {
        page: adw::NavigationPage,
        header: adw::HeaderBar,
        content: adw::ToolbarView,
        pages: adw::NavigationView,
        window: gtk::Window,
    },
}

impl Surface {
    pub fn new(parent: &impl IsA<gtk::Window>, title: &str, width: i32, height: i32) -> Self {
        if let Some((pages, window)) = PAGES.with(|host| host.borrow().clone()) {
            let header = adw::HeaderBar::new();
            let content = adw::ToolbarView::new();
            content.add_top_bar(&header);
            let page = adw::NavigationPage::builder()
                .title(title)
                .child(&content)
                .build();
            hide_header_in_full_screen(&pages, &page, &content, &window);
            return Self::Page {
                page,
                header,
                content,
                pages,
                window,
            };
        }
        let window = gtk::Window::builder()
            .title(title)
            .transient_for(parent)
            .modal(true)
            .default_width(width)
            .default_height(height)
            .build();
        Self::Window(window)
    }

    /// A page always covers the window.
    pub fn set_modal(&self, modal: bool) {
        if let Self::Window(w) = self {
            w.set_modal(modal);
        }
    }

    pub fn set_title(&self, title: &str) {
        match self {
            Self::Window(w) => w.set_title(Some(title)),
            Self::Page { page, .. } => page.set_title(title),
        }
    }

    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        match self {
            Self::Window(w) => w.set_child(Some(child)),
            // Wider content scrolls sideways instead of widening the page past the screen.
            Self::Page { content, .. } => {
                let scroller = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Automatic)
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .child(child)
                    .build();
                content.set_content(Some(&scroller));
            }
        }
    }

    /// For content that fits any width and scrolls by itself.
    pub fn set_child_fitted(&self, child: &impl IsA<gtk::Widget>) {
        match self {
            Self::Window(w) => w.set_child(Some(child)),
            Self::Page { content, .. } => content.set_content(Some(child)),
        }
    }

    /// What dialogs and file pickers opened from here attach to.
    pub fn window(&self) -> gtk::Window {
        match self {
            Self::Window(w) => w.clone(),
            Self::Page { window, .. } => window.clone(),
        }
    }

    pub fn add_controller(&self, controller: impl IsA<gtk::EventController>) {
        match self {
            Self::Window(w) => w.add_controller(controller),
            Self::Page { page, .. } => page.add_controller(controller),
        }
    }

    /// Asked before it closes, as a window's close-request is: `Stop` keeps it open.
    pub fn connect_close_request(&self, f: impl Fn() -> glib::Propagation + 'static) {
        let Self::Page {
            page,
            header,
            pages,
            ..
        } = self
        else {
            if let Self::Window(w) = self {
                w.connect_close_request(move |_| f());
            }
            return;
        };
        if !has_close_requests(page) {
            // The header's own back button and the swipe would leave without asking.
            page.set_can_pop(false);
            let back = gtk::Button::from_icon_name("go-previous-symbolic");
            let (weak_pages, weak_page) = (pages.downgrade(), page.downgrade());
            back.connect_clicked(move |_| {
                if let (Some(pages), Some(page)) = (weak_pages.upgrade(), weak_page.upgrade()) {
                    leave(&pages, &page);
                }
            });
            header.pack_start(&back);
            let gone = page.downgrade();
            on_popped(pages, page, move || {
                let gone = gone.upgrade();
                CLOSE_REQUESTS.with(|all| {
                    all.borrow_mut()
                        .retain(|(of, _)| of.upgrade().is_some_and(|of| Some(of) != gone))
                })
            });
        }
        CLOSE_REQUESTS.with(|all| all.borrow_mut().push((page.downgrade(), Rc::new(f))));
    }

    /// Runs `f` once it has closed, however that happened.
    pub fn connect_closed(&self, f: impl Fn() + 'static) {
        match self {
            // Not destroy: that waits for the last reference, and callers keep one until told.
            Self::Window(w) => {
                w.connect_unrealize(move |_| f());
            }
            Self::Page { page, pages, .. } => on_popped(pages, page, f),
        }
    }

    pub fn present(&self) {
        match self {
            Self::Window(w) => w.present(),
            Self::Page { page, pages, .. } => {
                if pages.visible_page().as_ref() != Some(page) {
                    pages.push(page);
                }
            }
        }
    }

    pub fn close(&self) {
        match self {
            Self::Window(w) => w.close(),
            Self::Page { page, pages, .. } => leave(pages, page),
        }
    }

    pub fn is_fullscreen(&self) -> bool {
        self.window().is_fullscreen()
    }

    /// Fills the screen, or gives it back; a page takes the main window with it.
    pub fn set_fullscreen(&self, wanted: bool) {
        let window = self.window();
        if wanted {
            window.fullscreen();
        } else {
            window.unfullscreen();
        }
    }
}
