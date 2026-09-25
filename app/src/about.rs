//! The About window and the hardware report behind its copy button, kept
//! together so `--debug-info` and the copy button can't come to differ.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_model::part;
use frameguin_wire::Bus;
use gtk4 as gtk;
use gtk4::{gdk, gdk_pixbuf, gio, glib, graphene, gsk};

use crate::APP_ID;
use crate::mapped;

const DAEMON_UNIT: &str = "frameguin-daemon.service";
/// Where a report is filed. `concat!` rather than a runtime format so the
/// About window's link and the issue `report_issue` opens are one expression.
const ISSUES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");
/// How long systemd gets to answer, in place of dbus-daemon's own 25 seconds.
const UNIT_STATE_TIMEOUT: Duration = Duration::from_secs(2);

/// One row of systemd's `ListUnitsByNames`. zvariant decodes positionally, so
/// the unread fields must stay, and stay in order: dropping one desyncs every
/// field after it rather than truncating the row.
#[derive(serde::Deserialize, zbus::zvariant::Type)]
#[zvariant(crate = "zbus::zvariant")]
struct UnitStatus {
    _name: String,
    _description: String,
    load_state: String,
    active_state: String,
    sub_state: String,
    _followed: String,
    _path: zbus::zvariant::OwnedObjectPath,
    _job_id: u32,
    _job_type: String,
    _job_path: zbus::zvariant::OwnedObjectPath,
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Systemd {
    /// One call where a resolved object path and three property reads would
    /// be four, and it answers for a unit that was never installed rather
    /// than failing on it — which is the case this is asked for.
    async fn list_units_by_names(&self, names: &[&str]) -> zbus::Result<Vec<UnitStatus>>;
}

/// What systemd makes of the daemon's unit: masked, failed and
/// never-installed are three different problems that leave the same files on
/// disk, or none at all. Asked on the connection that has just failed to
/// reach the daemon, since it is the same bus — and bounded, because
/// everything on this branch is asked when something is already not
/// answering, and a line saying the question went unanswered beats a full
/// report a minute after the click.
async fn unit_state(connection: &zbus::Connection) -> zbus::Result<String> {
    let ask = async {
        let manager = SystemdProxy::new(connection).await?;
        let units = manager.list_units_by_names(&[DAEMON_UNIT]).await?;
        let Some(unit) = units.into_iter().next() else {
            return Ok("not listed".to_string());
        };
        Ok(format!(
            "{}, {}, {}",
            unit.load_state, unit.active_state, unit.sub_state
        ))
    };
    glib::future_with_timeout(UNIT_STATE_TIMEOUT, ask)
        .await
        // A failure rather than a value: every other row in this report is
        // something a service said, and a wait that ran out is not one.
        .unwrap_or_else(|_| {
            Err(zbus::Error::Failure(format!(
                "no answer in {}s",
                UNIT_STATE_TIMEOUT.as_secs()
            )))
        })
}

/// Which of an install's service files are on disk, and whether the daemon the
/// activation file names is, for a report whose bus call failed. It cannot
/// separate installed-and-not-starting.
fn installed_service_files() -> String {
    // Named for the bus, not for GTK: the activation file is the bus name's,
    // and the two being the same string today is a coincidence of spelling.
    let activation = format!(
        "/usr/share/dbus-1/system-services/{}.service",
        frameguin_wire::BUS_NAME
    );
    let activated_daemon = std::fs::read_to_string(&activation).ok().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .map(str::to_string)
    });

    let present: Vec<String> = [activation, format!("/etc/systemd/system/{DAEMON_UNIT}")]
        .into_iter()
        .chain(activated_daemon)
        .filter(|path| std::path::Path::new(path).exists())
        .collect();
    if present.is_empty() {
        "none installed".to_string()
    } else {
        present.join(", ")
    }
}

/// What a hardware report needs, behind the About window's copy button, so
/// filing one does not require busctl.
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

    // Dialled rather than taken from the app's shared connection: this also
    // runs from `--debug-info`, where there is no app to have one, and a
    // report of whether the daemon answers wants asking afresh.
    let proxy = Bus::connect().await.map(|bus| bus.frameguin);
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
        // which is everything the two lines below would say.
        Err(e) => {
            out.push_str(&format!("daemon: {e}\n"));
            if let Ok(p) = &proxy {
                // Labelled with the unit asked about, which the answer does
                // not name: a drifted unit name would report "not-found" as
                // confidently as a missing install does.
                out.push_str(&line(DAEMON_UNIT, unit_state(p.inner().connection()).await));
            }
            out.push_str(&format!("service files: {}\n", installed_service_files()));
        }
    }

    // Only where the daemon answered: these go to the same service, so
    // asking again buys nothing but another activation timeout apiece — up
    // to 25 seconds each with a unit installed that never takes the name.
    if let Ok(p) = &proxy
        && build.is_ok()
    {
        // In the order the daemon answered, which is the journal's too.
        match p.get_devices().await {
            Ok(parts) => out.push_str(&part::listing(&parts)),
            Err(e) => out.push_str(&line("parts", Err(e))),
        }
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
    let about = adw::AboutDialog::builder()
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

    about.add_legal_section(
        "Framework",
        None,
        gtk::License::Custom,
        Some(&legal_markup(TRADEMARK_NOTICE)),
    );
    about.add_legal_section(
        "framework_lib",
        None,
        gtk::License::Custom,
        Some(&legal_markup(concat!(
            include_str!("../data/framework_lib/COPYING"),
            "\n",
            include_str!("../data/framework_lib/LICENSE.md")
        ))),
    );
    about.add_legal_section(
        "Tux",
        None,
        gtk::License::Custom,
        Some(&legal_markup(include_str!("../data/tux/COPYING"))),
    );

    let filling = about.clone();
    glib::spawn_future_local(async move {
        let info = debug_info().await;
        filling.set_debug_info(&info);
    });
    about.present(parent);
    // The dialog builds its contents on being presented.
    if let Some(icon) = app_icon(about.upcast_ref()) {
        peek_tux(&icon);
    }
}

// libadwaita renders a legal section as Pango markup in a wrapping label, and
// the license files included here are hard-wrapped.
fn legal_markup(text: &str) -> String {
    text.split("\n\n")
        .map(|paragraph| {
            paragraph
                .split_whitespace()
                .map(|word| {
                    let word = glib::markup_escape_text(word);
                    if word.starts_with("https://") {
                        format!("<a href=\"{word}\">{word}</a>")
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

const TRADEMARK_NOTICE: &str = "“Framework” and the gear logo are trademarks of Framework \
    Computer Inc. Frameguin is a community project, not affiliated with or endorsed by \
    Framework Computer Inc.";

const TUX_SVG: &[u8] = include_bytes!("../data/tux/tux.svg");
const TUX_DELAY: Duration = Duration::from_secs(10);
const TUX_CLIMB_MILLISECONDS: u32 = 900;
// Fractions of the icon's side, the hole's being where the icon's SVG draws it.
const HOLE_CENTRE: (f32, f32) = (12.003 / 24.0, 12.003 / 24.0);
const HOLE_RADII: (f32, f32) = (7.21 / 24.0, 7.407 / 24.0);
const TUX_HEIGHT: f32 = 0.742;
const TUX_PEEK_TOP: f32 = 0.43;

/// libadwaita gives no handle on the dialog's image of the icon.
fn app_icon(widget: &gtk::Widget) -> Option<gtk::Image> {
    if let Some(image) = widget.downcast_ref::<gtk::Image>()
        && image.icon_name().as_deref() == Some(APP_ID)
    {
        return Some(image.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(image) = app_icon(&widget) {
            return Some(image);
        }
        child = widget.next_sibling();
    }
    None
}

fn peek_tux(icon: &gtk::Image) {
    let peek = Rc::new(Peek {
        icon: icon.downgrade(),
        art: OnceCell::new(),
        animation: RefCell::default(),
    });

    let waiting = peek.clone();
    mapped::while_mapped(icon, move || {
        let peek = waiting.clone();
        mapped::Once::after(TUX_DELAY, move || {
            if peek.animation.borrow().is_none() {
                peek.toggle();
            }
        })
    });

    let double_click = gtk::GestureClick::new();
    double_click.connect_pressed(move |_, presses, _, _| {
        if presses == 2 {
            peek.toggle();
        }
    });
    icon.add_controller(double_click);
}

struct Peek {
    /// `Peek` is held by handlers on this image, which a strong reference
    /// would keep alive past the dialog's close.
    icon: glib::WeakRef<gtk::Image>,
    art: OnceCell<Option<Art>>,
    animation: RefCell<Option<adw::TimedAnimation>>,
}

impl Peek {
    fn toggle(&self) {
        let Some(icon) = self.icon.upgrade() else {
            return;
        };
        let Some(art) = self.art.get_or_init(|| Art::for_icon(&icon)).clone() else {
            return;
        };
        let interrupted = self.animation.take();
        // Skipping would jump Tux to the end the interrupted animation was
        // heading for.
        if let Some(interrupted) = &interrupted {
            interrupted.pause();
        }
        let from = interrupted.as_ref().map_or(0.0, AnimationExt::value);
        let (to, easing) = if interrupted.is_none_or(|going| going.value_to() < 0.5) {
            (1.0, adw::Easing::EaseOutBack)
        } else {
            (0.0, adw::Easing::EaseInBack)
        };
        let weak = self.icon.clone();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            if let Some(icon) = weak.upgrade() {
                icon.set_paintable(art.frame(value).as_ref());
            }
        });
        let animation = adw::TimedAnimation::new(&icon, from, to, TUX_CLIMB_MILLISECONDS, target);
        animation.set_easing(easing);
        animation.play();
        self.animation.replace(Some(animation));
    }
}

#[derive(Clone)]
struct Art {
    gear: gtk::IconPaintable,
    tux: gdk::Texture,
    side: f32,
}

impl Art {
    fn for_icon(icon: &gtk::Image) -> Option<Self> {
        let scale = icon.scale_factor();
        let (Ok(pixels), Ok(pixels_per_unit)) =
            (u16::try_from(icon.pixel_size()), u16::try_from(scale))
        else {
            return None;
        };
        let side = f32::from(pixels);
        let tux = tux_texture(side * f32::from(pixels_per_unit) * TUX_HEIGHT)?;
        let gear = gtk::IconTheme::for_display(&icon.display()).lookup_icon(
            APP_ID,
            &[],
            i32::from(pixels),
            scale,
            icon.direction(),
            gtk::IconLookupFlags::empty(),
        );
        Some(Self { gear, tux, side })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "an animation's progress needs no f64 precision"
    )]
    fn frame(&self, progress: f64) -> Option<gdk::Paintable> {
        let side = self.side;
        let snapshot = gtk::Snapshot::new();
        self.gear
            .snapshot(&snapshot, f64::from(side), f64::from(side));
        let (centre_x, centre_y) = HOLE_CENTRE;
        let (radius_x, radius_y) = HOLE_RADII;
        let corner = graphene::Size::new(radius_x * side, radius_y * side);
        snapshot.push_rounded_clip(&gsk::RoundedRect::new(
            graphene::Rect::new(
                (centre_x - radius_x) * side,
                (centre_y - radius_y) * side,
                2.0 * radius_x * side,
                2.0 * radius_y * side,
            ),
            corner,
            corner,
            corner,
            corner,
        ));
        let height = TUX_HEIGHT * side;
        #[expect(
            clippy::cast_precision_loss,
            reason = "texture sizes are far below f32's exact integer range"
        )]
        let width = height * self.tux.width() as f32 / self.tux.height() as f32;
        let hidden = centre_y + radius_y;
        let top = hidden + (TUX_PEEK_TOP - hidden) * progress as f32;
        snapshot.append_texture(
            &self.tux,
            &graphene::Rect::new((side - width) / 2.0, top * side, width, height),
        );
        snapshot.pop();
        snapshot.to_paintable(Some(&graphene::Size::new(side, side)))
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a pixel height well inside i32"
)]
fn tux_texture(height: f32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(TUX_SVG));
    let pixbuf = gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        -1,
        height.round() as i32,
        true,
        gio::Cancellable::NONE,
    )
    .ok()?;
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

#[cfg(test)]
mod tests {
    use super::legal_markup;

    #[test]
    fn lines_join_urls_link_and_markup_characters_escape() {
        assert_eq!(
            legal_markup(
                "Redrawn by Garrett LeSage & IFo Hancroft:\n   https://github.com/garrett/Tux\n\n<b>"
            ),
            "Redrawn by Garrett LeSage &amp; IFo Hancroft: \
             <a href=\"https://github.com/garrett/Tux\">https://github.com/garrett/Tux</a>\n\n&lt;b&gt;"
        );
    }

    #[test]
    fn the_icon_still_draws_the_hole_tux_is_clipped_to() {
        let icon = include_str!("../../data/icons/io.github.valeronm.Frameguin.svg");
        assert!(
            icon.contains("M12.003 19.41c-3.981 0-7.21-3.317-7.21-7.407s3.229-7.406 7.21-7.406")
        );
    }
}
