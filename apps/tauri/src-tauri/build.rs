fn main() {
    // The Windows executable icon is embedded from icons/icon.ico by tauri_build via a
    // generated resource. tauri_build does NOT register the icon files as rerun triggers,
    // so a changed icon is not re-embedded until build.rs reruns for some other reason
    // (stale resource.lib survives `cargo clean -p`). Track the icons explicitly so any
    // icon update forces the resource to recompile and the new icon to be embedded.
    for icon in [
        "icons/icon.ico",
        "icons/icon.png",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }
    tauri_build::build()
}
