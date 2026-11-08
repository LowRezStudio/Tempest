// The Windows build carries the Studio icon and version in its resources.
fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("resources/icon.ico");
    res.set("ProductName", "Tempest Studio Native");
    res.set("FileDescription", "Tempest Studio Native");
    if let Err(e) = res.compile() {
        println!("cargo:warning=the exe gets no icon: {e}");
    }
}
