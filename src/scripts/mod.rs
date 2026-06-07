//! FGO story script parser, message types, and translation pipeline.
//!
//! Modules:
//! - `parser`  : PEG grammar, Tag/Block, parse_script, speaker name extraction
//! - `message` : MessageEntry, ruby stripping, tag validation, JSON utilities
//! - `names`   : speaker name replacement, deharmonization, name scanning
//! - `export`  : flat-JSON export of dialogue + choices
//! - `import`  : import translated JSON back into .txt scripts
//! - `merge`   : merge message-only translated JSON into full-format originals
//! - `tools`   : directory comparison and deduplication
//! - `tests`   : parser unit tests

pub mod export;
pub mod import;
pub mod merge;
pub mod message;
pub mod names;
pub mod parser;
pub mod tools;

// Re-export the public API so main.rs can use `scripts::cmd_export` etc.
pub use export::cmd_export;
pub use import::cmd_import;
pub use merge::cmd_merge;
pub use names::cmd_deharmonize;
pub use names::cmd_scan_names;
pub use tools::cmd_compare;
pub use tools::cmd_dedup;
