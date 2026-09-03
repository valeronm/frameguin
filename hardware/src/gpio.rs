//! Processor pads through the kernel's GPIO character device, which reaches
//! the hardware without the EC and without addressing any device.
//!
//! Nothing here talks to the touch controller, because nothing here can: the
//! panel has no enable command to send it, so the control is a level on a
//! line and the device is never addressed. `docs/hardware.md` carries why.
//!
//! A pad is found by the name pinctrl gives it rather than by chip and
//! offset. Which `/dev/gpiochipN` a controller becomes depends on what else
//! registered a GPIO chip first — the EC registers one of its own — and the
//! Intel pinctrl device's ACPI name changes with each generation. The pad
//! name survives both.
//!
//! That name comes from a debugfs dump, which is the weak point here: it is
//! not a stable interface, it is parsed by position, and a kernel built
//! without debugfs or running without it mounted simply has no touchscreen
//! control. There is no better source — Intel's pinctrl leaves the character
//! device's own line names empty, so the stable interface cannot answer the
//! one question the name is needed for.

use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;

use crate::dmi;

/// The pad gating the touch controller, and the board that is true of.
///
/// Curated knowledge, per the probe rule: no side-effect-free exercise can
/// discover which pad drives what. Every Intel board of this generation has
/// a pad by this name and elsewhere it drives something else, so the board
/// is what makes the name mean the touchscreen — and a pad taken for the
/// wrong one would cut an unrelated rail rather than fail.
///
/// A second board needs both halves added together, and neither can be
/// guessed: `docs/hardware.md` gives the evidence for this pairing and the
/// method for establishing another.
///
/// Naming a board here also decides how its touch control behaves, which is
/// not obvious from this end: a pad found here wins over the panel's own
/// command, so a board added with a panel that implements one would move from
/// a control that reads its state back to a control that cannot. Check
/// [`crate::touchscreen::find`] before adding a board whose panel is not a
/// Himax.
///
/// Both halves stay on the path a write takes, unlike the rest of what
/// detection weighs. They are not a proxy standing in for the operation:
/// refusing here is refusing to drive an unknown line on hardware this
/// daemon cannot identify, which is the one failure that damages something
/// rather than returning an error.
const TOUCHSCREEN_PAD: &str = "GPP_B_18";
const TOUCHSCREEN_BOARD: &str = frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3;

/// What this daemon calls itself to the kernel while it holds a line. Shows
/// up as the line's consumer to anything else that looks.
const CONSUMER: &[u8] = b"frameguin";

const LINE_FLAG_OUTPUT: u64 = 1 << 3;
const ATTR_ID_OUTPUT_VALUES: u32 = 2;

/// `_IOWR(0xB4, nr, T)`, the request number `<linux/ioctl.h>` builds from a
/// direction, the payload's size and the driver's own type byte. Derived
/// from the struct rather than written out, so a layout that drifted from
/// the kernel's would be rejected by the ioctl instead of misread by it.
#[allow(
    clippy::cast_possible_truncation,
    reason = "sizes here are fixed by the uapi structs and far below u32"
)]
const fn iowr<T>(nr: u32) -> libc::c_ulong {
    let size = size_of::<T>() as u32;
    ((3 << 30) | (size << 16) | (0xB4 << 8) | nr) as libc::c_ulong
}

/// Take a line, and read the values of one already taken. Named so the test
/// below pins the numbers the calls actually carry rather than re-deriving
/// them from the same expression and agreeing with itself.
const GET_LINE: libc::c_ulong = iowr::<LineRequest>(0x07);
const GET_LINE_VALUES: libc::c_ulong = iowr::<LineValues>(0x0E);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LineAttribute {
    id: u32,
    padding: u32,
    /// A union in the kernel's header whose widest member is 8 bytes; only
    /// the one `id` names is read.
    value: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LineConfigAttribute {
    attr: LineAttribute,
    /// Which of the requested lines the attribute applies to, by bit.
    mask: u64,
}

#[repr(C)]
struct LineConfig {
    flags: u64,
    num_attrs: u32,
    padding: [u32; 5],
    attrs: [LineConfigAttribute; 10],
}

#[repr(C)]
struct LineRequest {
    offsets: [u32; 64],
    consumer: [u8; 32],
    config: LineConfig,
    num_lines: u32,
    event_buffer_size: u32,
    padding: [u32; 5],
    /// Filled in by the kernel: the fd that holds the request.
    fd: i32,
}

#[repr(C)]
struct LineValues {
    bits: u64,
    mask: u64,
}

impl LineRequest {
    /// One line, either pre-set to a level or asked for as the kernel found
    /// it. Which of the two decides the direction flag as well as the
    /// attribute, so the pair cannot be given separately and disagree — a
    /// request carrying a level and no direction would be applied to nothing.
    fn new(offset: u32, level: Option<bool>) -> Self {
        let mut offsets = [0; 64];
        offsets[0] = offset;
        let mut consumer = [0; 32];
        consumer[..CONSUMER.len()].copy_from_slice(CONSUMER);
        let mut attrs = [LineConfigAttribute::default(); 10];
        let (flags, num_attrs) = match level {
            Some(level) => {
                attrs[0] = LineConfigAttribute {
                    attr: LineAttribute {
                        id: ATTR_ID_OUTPUT_VALUES,
                        value: u64::from(level),
                        ..LineAttribute::default()
                    },
                    mask: 1,
                };
                (LINE_FLAG_OUTPUT, 1)
            }
            None => (0, 0),
        };
        Self {
            offsets,
            consumer,
            config: LineConfig {
                flags,
                num_attrs,
                padding: [0; 5],
                attrs,
            },
            num_lines: 1,
            event_buffer_size: 0,
            padding: [0; 5],
            fd: -1,
        }
    }
}

/// The one call into the kernel here. Every caller passes a `#[repr(C)]`
/// struct whose size the request number was derived from, so a mismatched
/// layout comes back as `ENOTTY` rather than reading past the struct.
fn ioctl<T>(fd: &impl AsRawFd, request: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    // SAFETY: `arg` is a live, uniquely borrowed T for the whole call, and
    // `request` encodes T's own size, so the kernel copies exactly the bytes
    // this struct has.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), request, std::ptr::from_mut(arg)) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A pad this daemon can address: which chip owns it and which line it is
/// there. Held rather than re-derived between the two questions asked of it,
/// so a reading and the write that follows cannot land on different lines.
pub struct Pad {
    chip: PathBuf,
    line: u32,
}

impl Pad {
    /// The pad pinctrl lists under `name`, if it is one this daemon may
    /// drive. None when debugfs is not mounted, the pad is not on this
    /// processor, or it is one this daemon must not touch — see [`usable`].
    fn locate(name: &str) -> Option<Self> {
        let needle = format!("({name})");
        for controller in fs::read_dir("/sys/kernel/debug/pinctrl").ok()?.flatten() {
            let Ok(pins) = fs::read_to_string(controller.path().join("pins")) else {
                continue;
            };
            let Some(listing) = pins.lines().find(|line| line.contains(&needle)) else {
                continue;
            };
            if !usable(listing) {
                return None;
            }
            // "pin <line> (<NAME>) ..." — the number the chip knows it by.
            let line = listing.split_whitespace().nth(1)?.parse().ok()?;
            let chip = chip_of(controller.file_name().to_str()?)?;
            return Some(Self { chip, line });
        }
        None
    }

    fn request(&self, level: Option<bool>) -> io::Result<OwnedFd> {
        let chip = fs::File::open(&self.chip)?;
        let mut request = LineRequest::new(self.line, level);
        ioctl(&chip, GET_LINE, &mut request)?;
        if request.fd < 0 {
            return Err(io::Error::other("kernel returned no line descriptor"));
        }
        // SAFETY: the kernel just handed this fd over and nothing else holds
        // it, so the OwnedFd is its only owner.
        Ok(unsafe { OwnedFd::from_raw_fd(request.fd) })
    }

    /// The level the pad is driving.
    ///
    /// Requested with no direction flag so the pad is left configured as it
    /// was found — asking for it as an output would be a write. What comes
    /// back is the level being driven rather than one read off the line; see
    /// `docs/hardware.md` for the condition that holds under.
    ///
    /// Doubles as the check that the pad is usable at all, which is why
    /// detection runs it rather than reading the line's info: it takes the
    /// same request the setter takes, so a chip that will not open and a line
    /// another driver holds both fail here exactly as they would there.
    /// Asking `GET_LINEINFO` instead would be a different call answering for
    /// the one that matters. Harmless only on a pad already in GPIO mode,
    /// which is what [`usable`] insists on.
    pub fn level(&self) -> io::Result<bool> {
        let line = self.request(None)?;
        let mut values = LineValues { bits: 0, mask: 1 };
        ioctl(&line, GET_LINE_VALUES, &mut values)?;
        Ok(values.bits & 1 != 0)
    }

    /// Drives the pad, and lets the line go.
    ///
    /// Releasing it is not a lapse: this daemon exits after five idle
    /// minutes, so a held line would come back on its own schedule and take
    /// the setting with it. Intel's pinctrl leaves `PADCFG` as the last
    /// requester set it, so the level outlives both the request and the
    /// process that made it.
    pub fn drive(&self, level: bool) -> io::Result<()> {
        self.request(Some(level))?;
        Ok(())
    }

    /// Opening the chip is where every operation begins, so a pad naming
    /// one that does not exist fails exactly where a real pad would.
    #[cfg(test)]
    pub(crate) fn unopenable() -> Self {
        Self {
            chip: PathBuf::from("/dev/frameguin-no-such-gpiochip"),
            line: 0,
        }
    }
}

/// Whether a pad's pinctrl listing describes one this daemon may take.
///
/// Two conditions, and the second is the one that is easy to miss. A pad the
/// firmware sealed carries `[LOCKED`, and taking it fails anyway — the value
/// of refusing here is that it comes back as a control this board does not
/// have rather than as a write that errored.
///
/// The mode matters because requesting a line is not the passive act it
/// looks like: gpiolib hands the request to pinctrl, which puts a pad into
/// GPIO mode if it is in some native one, disabling its output driver on the
/// way. On a pad already in GPIO mode — which is how firmware leaves the
/// touchscreen enable — that is a no-op and a read is genuinely
/// side-effect-free. On any other pad it would reconfigure the thing it was
/// only supposed to ask about, which on an enable line means cutting what it
/// enables. So a pad is only usable if the listing already says GPIO.
fn usable(listing: &str) -> bool {
    !listing.contains("[LOCKED") && listing.contains(" GPIO ")
}

/// The `/dev/gpiochipN` belonging to a pinctrl controller, by its ACPI name.
fn chip_of(controller: &str) -> Option<PathBuf> {
    fs::read_dir("/sys/bus/gpio/devices")
        .ok()?
        .flatten()
        .find_map(|chip| {
            // The link's own target names the controller, so reading it is
            // enough; resolving the path would walk every component to learn
            // nothing more.
            let owner = fs::read_link(chip.path()).ok()?;
            owner
                .to_str()?
                .contains(controller)
                .then(|| PathBuf::from("/dev").join(chip.file_name()))
        })
}

/// The touch controller's enable line, on the board where this pad is that.
/// Active high, so the pad's level reads directly as whether touch is on.
///
/// A touch panel in a mainboard of another generation answers None, and
/// deliberately: which pad carries the enable there is not known, and the
/// failure of a guess would be an unrelated line driven rather than an error
/// returned. A missing control is the recoverable half of that.
///
/// Whether a panel is behind the line is not asked here: that is
/// [`crate::touchscreen::find`]'s question, asked once at detection, and
/// what stays is what a write cannot be attempted without.
///
/// Resolved once and held: what a setter has to validate against is the line
/// itself, and [`Pad::request`] asks the kernel for it on every operation,
/// so a pad some driver has claimed since detection fails there rather than
/// being written on the strength of what was true at startup.
pub fn touchscreen() -> Option<Pad> {
    if dmi::product().as_deref() != Some(TOUCHSCREEN_BOARD) {
        return None;
    }
    Pad::locate(TOUCHSCREEN_PAD)
}

#[cfg(test)]
mod tests {
    use super::{
        ATTR_ID_OUTPUT_VALUES, GET_LINE, GET_LINE_VALUES, LINE_FLAG_OUTPUT, LineConfig,
        LineRequest, LineValues, usable,
    };

    /// The uapi structs, whose sizes the ioctl request numbers are built
    /// from — get one wrong and the kernel rejects every call. Pinned
    /// against `<linux/gpio.h>` so a mistake shows up here rather than as a
    /// control that never works.
    #[test]
    fn the_uapi_structs_are_the_sizes_the_kernel_expects() {
        assert_eq!(size_of::<LineConfig>(), 272);
        assert_eq!(size_of::<LineRequest>(), 592);
        assert_eq!(size_of::<LineValues>(), 16);
    }

    /// The numbers the calls carry, against the values the kernel's own
    /// header documents.
    #[test]
    fn the_request_numbers_are_the_documented_ones() {
        assert_eq!(GET_LINE, 0xc250_b407);
        assert_eq!(GET_LINE_VALUES, 0xc010_b40e);
    }

    /// What the request says it wants, which fails more quietly than the
    /// numbers above: a wrong ioctl number is rejected outright, where a
    /// wrong flag or attribute id is accepted and simply drives nothing.
    #[test]
    fn the_request_flags_are_the_documented_ones() {
        assert_eq!(LINE_FLAG_OUTPUT, 1 << 3);
        assert_eq!(ATTR_ID_OUTPUT_VALUES, 2);
    }

    /// Lines as `pinctrl-intel` writes them. The pad this daemon drives is
    /// the first; the rest are what must be refused, and the last two are
    /// the reason a mode check exists at all — taking a pad in a native mode
    /// would put it into GPIO and disable its output on the way, which on an
    /// enable line is the write the caller was only asking about.
    #[test]
    fn only_an_unlocked_pad_already_in_gpio_mode_is_usable() {
        let gpio = "pin 18 (GPP_B_18) 18:INTC10BC:04 GPIO 0x84000201 0x00000054 0x00000000";
        assert!(usable(gpio));
        assert!(!usable(&format!("{gpio} [LOCKED]")));
        assert!(!usable(
            "pin 44 (GPP_B_44) 44:INTC10BC:04 mode 1 0x44000102 0x00003000 0x00000000"
        ));
        assert!(!usable("pin 7 (GPP_A_7) 7:INTC10BC:00 not available"));
    }
}
