//! Resource download & decryption pipeline.
//!
//! Subcommands:
//! - `download <jp|cn>` — full pipeline
//! - `info <jp|cn>` — version info only
//! - `list <jp|cn>` — list script assets

mod asset_storage;
mod download;
mod parser;
mod pipeline;
mod version;

pub use pipeline::{cmd_download, cmd_info, cmd_list};
