fn main() {
    println!("cargo:rerun-if-changed=assets/phonemouse.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/phonemouse.ico");
        res.compile().expect("icône Windows");
    }
}
