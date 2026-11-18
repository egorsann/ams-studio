// Embeds the application icon into the Windows executable (does nothing on other platforms).
fn main() {
    println!("cargo:rerun-if-changed=assets/ams-studio.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/ams-studio.rc", embed_resource::NONE);
        // Windows gives the main thread 1 MB of stack (Linux 8 MB); the interface is built from deeply nested closures, so the
        // program asks for 16 MB. A stack overflow cannot be caught and closes the program without a word.
        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:16777216"),
            _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,16777216"),
        }
    }
}
