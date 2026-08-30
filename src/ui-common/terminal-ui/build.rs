fn main() {
    println!("cargo:rerun-if-changed=../../../src/gtk-app/locales");
    println!("cargo:rerun-if-changed=assets/terminal.gresource.xml");
    glib_build_tools::compile_resources(
        &["assets"],
        "assets/terminal.gresource.xml",
        "terminal.gresource",
    );
}
