
pub use ic_i18n::{set_lang, tr, trf};

pub fn register() {
    ic_i18n::register_locales!("../locales");
}
