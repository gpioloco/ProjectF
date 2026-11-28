fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/wte.ico");
        println!("cargo:rerun-if-changed=assets/wte.manifest");
        winresource::WindowsResource::new()
            .set_icon("assets/wte.ico")
            .set("ProductName", "Window Theme Engine")
            .set("FileDescription", "WTE · Window Theme Engine")
            .set("OriginalFilename", "wte.exe")
            .set_manifest_file("assets/wte.manifest")
            .compile()
            .expect("embed Windows resources");
    }
}
