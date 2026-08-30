use std::sync::Once;

static INIT: Once = Once::new();

fn ensure() {
    INIT.call_once(|| {
        ic_i18n::register_locales!("../../gtk-app/locales");
    });
}

pub fn tr(key: &str) -> String {
    ensure();
    ic_i18n::tr(key)
}

#[allow(dead_code)]
pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    ensure();
    ic_i18n::trf(key, args)
}
