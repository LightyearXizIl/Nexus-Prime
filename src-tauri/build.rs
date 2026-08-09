fn main() {
    tauri_build::build();

    // Make Tauri's generated Windows resource discoverable by the cfg(test)
    // link declaration in lib.rs. Regular binaries still use tauri-build's
    // normal resource link argument.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out_dir = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR for build scripts"),
        );
        println!("cargo:rustc-link-search=native={}", out_dir.display());
    }
}
