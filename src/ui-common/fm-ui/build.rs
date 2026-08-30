fn main() {
    println!("cargo:rerun-if-changed=../../../src/gtk-app/locales");
    println!("cargo:rerun-if-changed=assets/fm.gresource.xml");
    glib_build_tools::compile_resources(&["assets"], "assets/fm.gresource.xml", "fm.gresource");
}
