//! How this app spells a date, so a pack's manufacture and a firmware's
//! build cannot be written two ways one window apart.

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// An ISO date, with the time where the value carries one, as a reader says
/// it — and the value itself where it is not one.
///
/// The values that reach here carry no zone, so this spells what was
/// announced rather than converting it to the reader's clock.
#[must_use]
pub fn spelled(value: &str) -> String {
    read(value).unwrap_or_else(|| value.to_owned())
}

fn read(value: &str) -> Option<String> {
    let (date, time) = match value.split_once(' ') {
        Some((date, time)) => (date, Some(time)),
        None => (value, None),
    };
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    let day = day.parse::<u8>().ok()?;
    let month = MONTHS.get(month.parse::<usize>().ok()?.checked_sub(1)?)?;
    let Some(time) = time else {
        return Some(format!("{day} {month} {year}"));
    };
    let (hour, rest) = time.split_once(':')?;
    let minute = rest.split_once(':').map_or(rest, |(minute, _)| minute);
    Some(format!("{day} {month} {year}, {hour}:{minute}"))
}

#[cfg(test)]
mod tests {
    use super::spelled;

    #[test]
    fn a_date_alone_drops_the_leading_zero_it_was_padded_with() {
        assert_eq!(spelled("2025-03-04"), "4 March 2025");
    }

    #[test]
    fn a_date_and_time_keeps_the_hour_and_the_minute() {
        assert_eq!(spelled("2026-05-26 04:34:57"), "26 May 2026, 04:34");
    }

    #[test]
    fn a_value_of_no_date_shape_is_left_as_it_was_announced() {
        assert_eq!(spelled("STATIC_VERSION_DATE"), "STATIC_VERSION_DATE");
        assert_eq!(spelled("2026-13-01"), "2026-13-01");
        assert_eq!(spelled(""), "");
    }
}
