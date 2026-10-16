//! Trivial de-obfuscation for a handful of upstream endpoint hosts and a
//! package secret, so their plaintext does not surface in code search
//! (Google / GitHub). This is NOT security — the binary decodes at runtime and
//! the values are plainly visible in memory and on the wire. It only keeps the
//! literal strings out of the checked-in source.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn sextet(c: u8) -> u32 {
    match ALPHABET.iter().position(|&a| a == c) {
        Some(p) => p as u32,
        None => panic!("obf: invalid base64 byte {c:#x}"),
    }
}

/// Decode a standard-base64 `&str` (padding optional, ASCII whitespace
/// ignored) to a UTF-8 `String`. Panics on malformed input — every caller
/// passes a compile-time constant covered by a round-trip test.
pub fn reveal(b64: &str) -> String {
    let clean: Vec<u8> = b64
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(clean.len() / 4 * 3 + 3);
    for chunk in clean.chunks(4) {
        let n = chunk.len();
        assert!(n >= 2, "obf: truncated base64");
        let mut buf = 0u32;
        for &c in chunk {
            buf = (buf << 6) | sextet(c);
        }
        buf <<= 6 * (4 - n) as u32;
        // n groups of 6 bits → n-1 decoded bytes (2→1, 3→2, 4→3).
        let bytes = [(buf >> 16) as u8, (buf >> 8) as u8, buf as u8];
        out.extend_from_slice(&bytes[..n - 1]);
    }
    String::from_utf8(out).expect("obf: decoded bytes are not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_round_trips() {
        // "hi" / "man" / "hello" cover the 2/3/4-char final-group cases.
        assert_eq!(reveal("aGk="), "hi");
        assert_eq!(reveal("bWFu"), "man");
        assert_eq!(reveal("aGVsbG8="), "hello");
        // Padding is optional and whitespace is ignored.
        assert_eq!(reveal("aGk"), "hi");
        assert_eq!(reveal("aGVs bG8="), "hello");
    }
}
