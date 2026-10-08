fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Native Tauri/dialog code can also be linked into Cargo's library test
        // executable. Every such executable needs the same Common Controls v6
        // activation as the desktop app; otherwise TaskDialogIndirect is absent.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // tauri-build already links a complete RT_MANIFEST into binary targets.
        // Keep that resource instead of asking LINK to generate a duplicate
        // manifest with the same resource ID. The current Tauri default contains
        // only the Common Controls dependency, not extra DPI/OS declarations.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }
}
