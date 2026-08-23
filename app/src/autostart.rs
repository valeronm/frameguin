//! The session's autostart entry, behind the window's "Start at login" switch.

use std::fs;

use gtk4::glib;

use crate::APP_ID;

pub(crate) fn entry_path() -> std::path::PathBuf {
    glib::user_config_dir().join(format!("autostart/{APP_ID}.desktop"))
}

/// Names the binary rather than a path, so the entry survives a move between
/// install prefixes; `TryExec` lets the session skip it once Frameguin is gone,
/// which no uninstaller can do for a file in someone's home directory.
fn entry() -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Frameguin\n\
         Comment=Framework laptop controls in the tray\n\
         TryExec=frameguin\n\
         Exec=frameguin --gapplication-service\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         NoDisplay=true\n"
    )
}

pub(crate) fn set(enabled: bool) -> std::io::Result<()> {
    let path = entry_path();
    if enabled {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, entry())
    } else {
        match fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}
