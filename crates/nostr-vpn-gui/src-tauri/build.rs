fn main() {
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=NVPN_GUI_TARGET={target}");
    }
    tauri_build::build();
}
