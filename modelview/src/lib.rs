//! How the app shows the machine: the words for every value, the presets a
//! control offers with the row each one sits on, and the names for the
//! curated facts `frameguin_model` holds. A front end takes its words and
//! rows from here, so two of them cannot disagree about what a value is
//! called or what a row sends.

pub mod battery;
pub mod date;
pub mod extender;
pub mod rows;
pub mod words;
