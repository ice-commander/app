fn main() {
    println!("cargo:rerun-if-changed=../../../src/gtk-app/locales");
    println!("cargo:rerun-if-changed=assets/process.gresource.xml");
    glib_build_tools::compile_resources(
        &["assets"],
        "assets/process.gresource.xml",
        "process.gresource",
    );
}
