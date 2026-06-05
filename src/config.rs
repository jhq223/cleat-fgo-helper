//! Server configurations and cryptographic key sources for JP/CN.

/// Server configuration for a single region.
pub struct ServerConfig {
    /// "jp" or "cn" — determines key derivation and URL construction.
    pub server_type: &'static str,
    /// Version info endpoint
    pub version_url: &'static str,
    /// Regex to extract version from upgrade response (JP only)
    pub version_regex: Option<&'static str>,
    /// CDN base URL for AssetStorage and bundles
    pub cdn_base: &'static str,
    /// Platform string ("Android")
    pub platform: &'static str,
    /// Whether script encryption uses BZip2 (CN: true, JP/EN: false)
    pub use_bzip2: bool,
    /// Template for CN network config URL
    pub network_config_tpl: Option<&'static str>,
}

pub const JP_CONFIG: ServerConfig = ServerConfig {
    server_type: "jp",
    version_url: "https://game.fate-go.jp/gamedata/top?appVer=0.0",
    version_regex: Some("新ver.：(.*?)、現"),
    cdn_base: "https://cdn.data.fate-go.jp/AssetStorages",
    platform: "Android",
    use_bzip2: false,
    network_config_tpl: None,
};

pub const CN_CONFIG: ServerConfig = ServerConfig {
    server_type: "cn",
    version_url: "https://line1-h5-pc-api.biligame.com/game/detail/content?game_base_id=49",
    version_regex: None,
    cdn_base: "", // CN uses dynamic CDN from network_config
    platform: "Android",
    use_bzip2: true,
    network_config_tpl: Some(
        "http://line1-s1-bili-fate.bilibiligame.net/rongame_beta/rgfate/60_member/network/network_config_android_{version}.json",
    ),
};

pub fn get_config(server: &str) -> &'static ServerConfig {
    match server.to_uppercase().as_str() {
        "JP" => &JP_CONFIG,
        "CN" => &CN_CONFIG,
        _ => panic!("Unknown server: {server}"),
    }
}

// ── Key sources ──

pub const KEY_SOURCE_JP_STAGE: &[u8; 64] =
    b"kzdMtpmzqCHAfx00saU1gIhTjYCuOD1JstqtisXsGYqRVcqrHRydj3k6vJCySu3g";
pub const KEY_SOURCE_JP_BASE: &[u8; 64] =
    b"PFBs0eIuunoxKkCcLbqDVerU1rShhS276SAL3A8tFLUfGvtz3F3FFeKELIk3Nvi4";

pub const KEY_SOURCE_CN_STAGE: &[u8; 64] =
    b"d3b13d9093cc6b457fd89766bafa1626ee2ef76626d49ce0d424f4156231ce56";
pub const KEY_SOURCE_CN_BASE: &[u8; 64] =
    b"5ec7ce0fddc50bca9f82b8338b9135c69e0e9e169648df69054dcb96553598e6";

/// AssetBundle gamedata key (32 bytes, UTF-8)
pub const ASSET_BUNDLE_KEY: &[u8; 32] = b"W0Juh4cFJSYPkebJB9WpswNF51oa6Gm7";

/// MD5 salt for Audio/Movie filename hashing
pub const MD5_SALT: &[u8; 8] = b"pN6ds2Bg";

/// Derive keys for a given server.
/// Returns (stage_data, stage_top, base_data, base_top) — each 32 bytes.
pub fn derive_keys(server: &str) -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32]) {
    use crate::crypto::keys;

    match server.to_uppercase().as_str() {
        "JP" => {
            let (sd, st) = keys::derive_byte_interleave(KEY_SOURCE_JP_STAGE);
            let (bd, bt) = keys::derive_4byte_interleave(KEY_SOURCE_JP_BASE);
            (sd, st, bd, bt)
        }
        "CN" => {
            let (sd, st) = keys::derive_4byte_interleave(KEY_SOURCE_CN_STAGE);
            let (bd, bt) = keys::derive_byte_interleave(KEY_SOURCE_CN_BASE);
            (sd, st, bd, bt)
        }
        _ => panic!("Unknown server: {server}"),
    }
}
