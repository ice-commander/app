/// What the desktop and the phone do differently.
pub trait Platform {
    fn app_id(&self) -> &'static str {
        "com.icecommander.gtkapp"
    }
    /// One panel at a time, touch first.
    fn mobile(&self) -> bool;
    /// Back with nothing left to go back to. True: the platform put the app away itself and the
    /// window must stay; false: the window closes.
    fn put_away(&self, _window: &gtk::Window) -> bool {
        false
    }
    /// Local panels are real paths, so the phone needs All-files access first.
    fn ensure_storage_access(&self, _window: &gtk::Window) {}
    /// Once the window has an activity: native libraries that ask Android for its services
    /// (audio) find them from now on.
    fn attach_services(&self, _window: &gtk::Window) {}
    /// While a film is on: the screen does not dim and lock under it.
    fn keep_screen_on(&self, _window: &gtk::Window, _on: bool) {}

    fn paint_system_bars(&self, _window: &gtk::Window, _argb: i32) {}
}

/// The desktop. `IC_MOBILE=1` gives it the phone's layout, for trying it without a phone.
pub struct Desktop {
    mobile: bool,
}

impl Desktop {
    pub fn from_env() -> Self {
        Self {
            mobile: std::env::var_os("IC_MOBILE").is_some_and(|v| v == "1"),
        }
    }
}

impl Platform for Desktop {
    fn mobile(&self) -> bool {
        self.mobile
    }
}
