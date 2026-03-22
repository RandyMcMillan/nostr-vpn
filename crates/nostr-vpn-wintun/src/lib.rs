#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use anyhow::{Context, Result, anyhow};

#[cfg(target_os = "windows")]
pub use wintun::Wintun;

#[cfg(target_os = "windows")]
pub fn resolve_wintun_dll_path() -> Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let mut candidates = vec![dir.join("wintun.dll")];
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("wintun.dll"));
            candidates.push(parent.join("resources").join("wintun.dll"));
            candidates.push(parent.join("Resources").join("wintun.dll"));
            candidates.push(parent.join("resources").join("binaries").join("wintun.dll"));
            candidates.push(parent.join("Resources").join("binaries").join("wintun.dll"));
            if let Some(grandparent) = parent.parent() {
                candidates.push(grandparent.join("Resources").join("wintun.dll"));
                candidates.push(
                    grandparent
                        .join("Resources")
                        .join("binaries")
                        .join("wintun.dll"),
                );
            }
        }

        for candidate in candidates {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let built_path = PathBuf::from(env!("NOSTR_VPN_WINTUN_DLL_SOURCE"));
    if built_path.is_file() {
        return Ok(built_path);
    }

    Err(anyhow!(
        "wintun.dll not found next to executable or in build output"
    ))
}

#[cfg(target_os = "windows")]
pub fn load_wintun() -> Result<Wintun> {
    let dll_path = resolve_wintun_dll_path()?;
    // The path is constrained to a bundled wintun.dll location we control.
    unsafe { wintun::load_from_path(&dll_path) }
        .with_context(|| format!("failed to load wintun.dll from {}", dll_path.display()))
}
