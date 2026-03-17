fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        // Give it the ID "app-icon" so we can reference it in Rust code!
        res.set_icon_with_id("sp2p.ico", "app-icon");
        res.compile().unwrap();
    }
}
