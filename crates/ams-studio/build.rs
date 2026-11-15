// Embeds the application icon into the Windows executable (does nothing on other platforms).
fn main() {
    println!("cargo:rerun-if-changed=assets/ams-studio.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/ams-studio.rc", embed_resource::NONE);
    }
}
