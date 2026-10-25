fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS") == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("embed Windows resources");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
