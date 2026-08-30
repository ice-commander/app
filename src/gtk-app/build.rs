fn main() {
    println!("cargo:rerun-if-changed=assets/resources.gresource.xml");
    println!("cargo:rerun-if-changed=assets/webui/bundle.js");
    println!("cargo:rerun-if-changed=assets/webui/style.css");
    glib_build_tools::compile_resources(
        &["assets"],
        "assets/resources.gresource.xml",
        "icecommander.gresource",
    );

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/win32-icon.ico");

        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity version="1.0.0.0" name="MyApplication.App"/>
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
        </requestedPrivileges>
    </security>
</trustInfo>
<dependency>
    <dependentAssembly>
        <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
</dependency>
</assembly>
"#);

        res.compile().expect("Failed to compile Windows resources");
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let src_lib_path = if target_os == "windows" {
        std::path::PathBuf::from("../../artifacts/gtk4-win32-x64/pdfium.dll")
    } else if target_os == "macos" {
        std::path::PathBuf::from("../../artifacts/libpdfium.dylib")
    } else {
        std::path::PathBuf::from("../../artifacts/libpdfium.so")
    };

    if src_lib_path.exists() {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let out_path = std::path::Path::new(&out_dir);
        if let Some(target_dir) = out_path.ancestors().nth(4) {
            let dest_filename = if target_os == "windows" {
                "pdfium.dll"
            } else if target_os == "macos" {
                "libpdfium.dylib"
            } else {
                "libpdfium.so"
            };
            let _ = std::fs::copy(&src_lib_path, target_dir.join(dest_filename));
            if let Some(deps_dir) = out_path.ancestors().nth(3) {
                let _ = std::fs::copy(&src_lib_path, deps_dir.join(dest_filename));
            }
        }
    }
}
