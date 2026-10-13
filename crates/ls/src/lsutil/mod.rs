//! tsgo `ls/lsutil`: the user preferences (their types, parsing and
//! configuration form generated from tsgo's schema) and the format settings.

mod format_code_options;
mod user_preferences;
mod user_preferences_generated;

pub use format_code_options::*;
pub use user_preferences::*;
pub use user_preferences_generated::*;
