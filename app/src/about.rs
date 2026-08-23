//! The About window and the hardware report behind its copy button, kept
//! together so `--debug-info` and the copy button can't come to differ.

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::glib;

use crate::board::dmi;
use crate::{APP_ID, daemon_proxy};

/// What a hardware report needs, behind the About window's copy button, so
/// filing one does not require busctl. Both binaries report where they ran
/// from: a mixed install has the app under one prefix and the daemon under
/// another, and no version comparison would show it when the two trees hold
/// the same release.
#[expect(
    clippy::format_push_string,
    reason = "the allocation is immaterial in a report built once to fill a dialog"
)]
pub(crate) async fn debug_info() -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| "unknown".into());
    let mut out = format!(
        "Frameguin {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        exe.display()
    );

    let line = |name: &str, value: zbus::Result<String>| match value {
        Ok(v) => format!("{name}: {v}\n"),
        Err(e) => format!("{name}: unavailable ({e})\n"),
    };

    // The two binaries first and adjacent, since comparing their paths is
    // what the report is read for; the hardware they found follows.
    let proxy = daemon_proxy().await;
    match &proxy {
        Ok(p) => out.push_str(&line(
            "daemon",
            p.get_build().await.map(|(v, path)| format!("{v} ({path})")),
        )),
        Err(e) => out.push_str(&format!("daemon: unreachable ({e})\n")),
    }

    out.push_str(&format!(
        "board: {}\nBIOS: {}\n",
        dmi("product_name"),
        dmi("bios_version")
    ));

    if let Ok(p) = &proxy {
        out.push_str(&line("EC", p.get_ec_version().await));
        out.push_str(&line(
            "capabilities",
            p.get_capabilities()
                .await
                // Variant names, not the wire's: the kebab spelling lives in
                // one serde attribute, and a second copy to format from here
                // is the drift these types exist to remove.
                .map(|caps| format!("{caps:?}")),
        ));
    }
    out
}

pub(crate) fn show(parent: Option<&gtk::Window>) {
    let about = adw::AboutWindow::builder()
        .application_icon(APP_ID)
        .application_name("Frameguin")
        .developer_name("Valerii Myronov")
        .version(env!("CARGO_PKG_VERSION"))
        .license_type(gtk::License::MitX11)
        // Setting the comments property would create a Details page and move
        // the website link onto it, off the main page.
        .website(env!("CARGO_PKG_HOMEPAGE"))
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"))
        .debug_info_filename("frameguin-debug-info.txt")
        // Placeholder rather than empty: libadwaita hides the Troubleshooting
        // page entirely when debug info is blank, and this fills in later.
        .debug_info("collecting…")
        .build();
    about.set_transient_for(parent);

    let filling = about.clone();
    glib::spawn_future_local(async move {
        let info = debug_info().await;
        filling.set_debug_info(&info);
    });
    about.present();
}
