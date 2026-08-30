mod i18n;

pub mod fs_ftp;
pub mod fs_sftp;
pub mod fs_webdav;

pub mod archive_rpc;
pub mod dialogs;
pub mod drives_root_rpc;
pub mod ftp_rpc;
pub mod local_rpc;
pub mod sftp_rpc;
pub mod utils;
pub mod webdav_rpc;

pub fn set_connect_timeout_secs(secs: u64) {
    common::set_connect_timeout_secs(secs);
}

pub fn set_request_timeout_secs(secs: u64) {
    common::set_request_timeout_secs(secs);
}

pub fn net_content_wait() -> fm_core::rpc::ContentWait {
    match common::request_timeout() {
        None => fm_core::rpc::ContentWait::Infinite,
        Some(d) => fm_core::rpc::ContentWait::Bounded(d),
    }
}

pub fn hostname() -> String {
    static HOSTNAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOSTNAME
        .get_or_init(|| {
            #[cfg(feature = "gtk")]
            let h = gtk::glib::host_name().to_string();
            #[cfg(not(feature = "gtk"))]
            let h = std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("HOSTNAME").ok())
                .unwrap_or_default();
            if h.is_empty() { "Computer".to_string() } else { h }
        })
        .clone()
}

pub(crate) fn spawn_local<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    #[cfg(feature = "gtk")]
    {
        gtk::glib::spawn_future_local(fut);
    }
    #[cfg(not(feature = "gtk"))]
    {
        tokio::task::spawn_local(fut);
    }
}
