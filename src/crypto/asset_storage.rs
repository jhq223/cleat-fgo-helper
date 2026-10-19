//! AssetStorage uses the legacy script codec with a larger metadata limit.
pub fn decrypt(text: &str, key: &[u8; 32], iv: &[u8; 32]) -> Result<String, String> {
    super::script::decode(text, key, iv, true, 64 * 1024 * 1024)
}
