//! The About window and the hardware report behind its copy button, kept
//! together so `--debug-info` and the copy button can't come to differ.

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use crate::board::dmi;
use crate::{APP_ID, daemon_proxy};

/// The unit an install writes, one spelling for the two places a package and
/// a tarball put it.
const DAEMON_UNIT: &str = "frameguin-daemon.service";
/// Where a report is filed. `concat!` rather than a runtime format so the
/// About window's link and the issue `report_issue` opens are one expression.
const ISSUES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");

/// Which of an install's service files are actually on disk, for a report
/// whose bus call failed: it separates nothing installed from a package
/// install, a tarball install, or one of each shadowing the other. What it
/// cannot separate is installed-and-not-starting — systemd is the only
/// witness to that, and this asks the filesystem.
///
/// The libexec candidates include one derived from this binary's own prefix,
/// because `install.sh` takes the prefix as a parameter — a `PREFIX=/opt`
/// install would otherwise be reported as no install at all.
fn installed_service_files() -> String {
    // Named for the bus, not for GTK: the activation file is the bus name's,
    // and the two being the same string today is a coincidence of spelling.
    let activation = format!(
        "/usr/share/dbus-1/system-services/{}.service",
        frameguin_wire::BUS_NAME
    );
    let own_libexec = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let prefix = exe.parent()?.parent()?;
            Some(
                prefix
                    .join("libexec/frameguin-daemon")
                    .display()
                    .to_string(),
            )
        })
        // The two fixed libexec paths already cover /usr, so what is left is
        // a prefix nothing else here would have looked in.
        .filter(|path| !path.starts_with("/usr/"));

    let candidates = [
        activation,
        format!("/etc/systemd/system/{DAEMON_UNIT}"),
        format!("/usr/lib/systemd/system/{DAEMON_UNIT}"),
        "/usr/libexec/frameguin-daemon".to_string(),
        "/usr/local/libexec/frameguin-daemon".to_string(),
    ];
    let present: Vec<String> = candidates
        .into_iter()
        .chain(own_libexec)
        .filter(|path| std::path::Path::new(path).exists())
        .collect();
    if present.is_empty() {
        "none installed".to_string()
    } else {
        present.join(", ")
    }
}

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
    let build = match &proxy {
        Ok(p) => p
            .get_build()
            .await
            .map(|(version, path)| format!("{version} ({path})"))
            .map_err(|e| format!("unavailable ({e})")),
        Err(e) => Err(format!("unreachable ({e})")),
    };
    match &build {
        Ok(v) => out.push_str(&format!("daemon: {v}\n")),
        // A daemon that answered has just named its own version and path,
        // which is everything the inventory would say. One that did not
        // leaves a report unable to tell an absent install from a present
        // one, which is the whole question at that point.
        Err(e) => out.push_str(&format!(
            "daemon: {e}\nservice files: {}\n",
            installed_service_files()
        )),
    }

    out.push_str(&format!(
        "board: {}\nBIOS: {}\n",
        dmi("product_name"),
        dmi("bios_version")
    ));

    // Only where the daemon answered: these go to the same service, so
    // asking again buys nothing but another activation timeout apiece — up
    // to 25 seconds each with a unit installed that never takes the name.
    if let Ok(p) = &proxy
        && build.is_ok()
    {
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

/// Opens a new issue with the report already in its body. Here rather than
/// with the window that offers it, for the reason this module exists: what a
/// report says has one source, and filing one is a third consumer of it
/// beside `--debug-info` and the copy button. The state it is offered from is
/// the one where the app cannot answer for itself, so the body is gathered
/// rather than asked for.
pub(crate) async fn report_issue() -> Result<(), glib::Error> {
    let body = format!(
        "### What happened\n\n\n### Debug info\n\n```\n{}```\n",
        debug_info().await
    );
    let url = format!(
        "{ISSUES_URL}/new?body={}",
        glib::Uri::escape_string(&body, None, true)
    );
    gio::AppInfo::launch_default_for_uri(&url, gio::AppLaunchContext::NONE)
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
        .issue_url(ISSUES_URL)
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
