fn main() {
    println!("cargo:rerun-if-changed=../../../src/gtk-app/locales");
    println!("cargo:rerun-if-changed=assets/registry.gresource.xml");
    glib_build_tools::compile_resources(
        &["assets"],
        "assets/registry.gresource.xml",
        "registry.gresource",
    );
}
