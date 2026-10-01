fn main() {
    println!("cargo:rerun-if-changed=assets/resources.gresource.xml");
    println!("cargo:rerun-if-changed=assets/webui/bundle.js");
    println!("cargo:rerun-if-changed=assets/webui/style.css");

    for asset in ["assets/webui/bundle.js", "assets/webui/style.css"] {
        if !std::path::Path::new(asset).exists() {
            panic!("{asset} is generated and missing: run `npm run build-web-app` first");
        }
    }
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
}
