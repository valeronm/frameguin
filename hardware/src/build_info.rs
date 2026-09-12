//! What the EC's build-info string says the firmware is, apart from
//! [`crate::ec`] so the reading of it is testable without an EC.

/// A firmware as its build-info string describes it.
pub(crate) struct Build {
    pub(crate) version: String,
    pub(crate) built: String,
    pub(crate) builder: String,
}

/// The stamp is itself two space-separated fields. A string of another shape
/// is kept whole as the version, a firmware being worth showing as it
/// spelled itself even where nothing here recognises the shape.
#[must_use]
pub(crate) fn parse(info: &str) -> Build {
    let fields: Vec<&str> = info.split_whitespace().collect();
    match fields.as_slice() {
        [version, built @ .., builder] if !built.is_empty() => Build {
            version: (*version).to_owned(),
            built: built.join(" "),
            builder: (*builder).to_owned(),
        },
        _ => Build {
            version: info.trim().to_owned(),
            built: String::new(),
            builder: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn a_build_stamp_of_a_date_and_a_time_stays_one_field() {
        let build = parse("sakura-3.0.2-cf48815 2026-05-26 04:34:57 lotus@ip-172-26-3-226");
        assert_eq!(build.version, "sakura-3.0.2-cf48815");
        assert_eq!(build.built, "2026-05-26 04:34:57");
        assert_eq!(build.builder, "lotus@ip-172-26-3-226");
    }

    #[test]
    fn a_reproducible_build_names_a_stamp_of_one_field() {
        let build = parse("sakura-3.0.2-cf48815 STATIC_VERSION_DATE reproducible@build");
        assert_eq!(build.version, "sakura-3.0.2-cf48815");
        assert_eq!(build.built, "STATIC_VERSION_DATE");
        assert_eq!(build.builder, "reproducible@build");
    }

    #[test]
    fn a_string_of_no_known_shape_is_the_version_whole() {
        let build = parse("  3.0.2  ");
        assert_eq!(build.version, "3.0.2");
        assert_eq!(build.built, "");
        assert_eq!(build.builder, "");
    }
}
