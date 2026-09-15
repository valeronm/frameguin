//! The chassis open switch: one read, and the words a reader needs for it.

use std::rc::Rc;

use frameguin_wire::{ChassisControl, ChassisState, DeviceResult as Result};

use super::present;

pub struct Chassis<C> {
    control: Rc<C>,
}

impl<C: ChassisControl> Chassis<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.state().await)?.map(|_| Self {
            control: control.clone(),
        }))
    }

    pub async fn read(&self) -> Result<ChassisState> {
        self.control.state().await
    }
}

#[must_use]
pub fn state_label(open: bool) -> &'static str {
    if open { "Open" } else { "Closed" }
}

#[must_use]
pub fn open_now_label(open: bool) -> &'static str {
    if open { "Yes" } else { "No" }
}

/// The EC's counts stop at this value.
const COUNT_CEILING: u8 = 255;

#[must_use]
pub fn times_label(count: u8) -> String {
    if count == COUNT_CEILING {
        format!("{COUNT_CEILING} or more")
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{open_now_label, state_label, times_label};

    #[test]
    fn a_count_the_ec_stopped_at_reads_as_a_floor() {
        assert_eq!(times_label(254), "254");
        assert_eq!(times_label(255), "255 or more");
    }

    #[test]
    fn the_switch_is_worded_both_ways() {
        assert_eq!(state_label(true), "Open");
        assert_eq!(state_label(false), "Closed");
        assert_eq!(open_now_label(true), "Yes");
        assert_eq!(open_now_label(false), "No");
    }
}
