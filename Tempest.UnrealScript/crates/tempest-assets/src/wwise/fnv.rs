//! FNV-1 (32-bit), Wwise's hash for object names and the codebook hash of a
//! Vorbis setup packet (`uHashCodebook` in `AkVorbisInfo`).

pub fn fnv1_32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h = h.wrapping_mul(0x01000193) ^ b as u32;
    }
    h
}

/// The id Wwise gives a named object (event, bank, switch…): FNV-1 of the
/// lower-case name.
pub fn name_id(name: &str) -> u32 {
    fnv1_32(name.to_ascii_lowercase().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hashes() {
        // Wwise's own Init bank.
        assert_eq!(name_id("Init"), 0x50c63a23);
        assert_eq!(name_id("WWB_CINE_Abyss_SFX"), 0x20f7f114);
    }
}
