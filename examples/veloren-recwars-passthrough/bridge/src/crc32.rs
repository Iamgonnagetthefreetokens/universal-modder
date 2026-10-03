//! CRC-32 (IEEE), the same polynomial and bit order as zlib's `crc32`, so a payload written here
//! validates against the Python reference and vice versa.

const POLY: u32 = 0xEDB8_8320;

/// CRC-32 of `data`, identical to `zlib.crc32(data)` in Python.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (POLY & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn known_vectors() {
        // The three values every CRC-32 implementation is checked against.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }
}
