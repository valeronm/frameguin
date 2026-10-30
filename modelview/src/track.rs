//! A reading laid along a line among what it is judged by, every position
//! in thousandths of the line's length.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Caution,
    Shutdown,
    /// Where the reading is meant to sit.
    Target,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Track {
    pub band: Option<(u16, u16)>,
    pub marks: Vec<(u16, Mark)>,
    pub dot: Option<u16>,
    /// In the order a label with no room gives way in.
    pub ticks: Vec<(u16, String)>,
    /// What the marks stand for, in words.
    pub description: String,
}

impl Track {
    pub fn placed(&self, mark: Mark) -> impl Iterator<Item = u16> + '_ {
        self.marks
            .iter()
            .filter(move |(_, placed)| *placed == mark)
            .map(|(position, _)| *position)
    }
}

/// A value past either end sits at that end. `end` has to be past `start`.
pub(crate) fn position(value: u32, start: u32, end: u32) -> u16 {
    let span = end - start;
    let past = value.clamp(start, end) - start;
    u16::try_from((u64::from(past) * 1000 + u64::from(span / 2)) / u64::from(span)).unwrap_or(1000)
}
