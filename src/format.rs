/// Format bytes into a standard 16-byte width hexdump
pub fn hexdump(data: &[u8]) -> String {
    let mut out = String::new();
    let chunk_size = 16;

    for (offset, chunk) in data.chunks(chunk_size).enumerate() {
        let abs_offset = offset * chunk_size;
        out.push_str(&format!("{:08x}  ", abs_offset));

        // Hex section
        for i in 0..chunk_size {
            if i < chunk.len() {
                out.push_str(&format!("{:02x} ", chunk[i]));
            } else {
                out.push_str("   ");
            }
            if i == 7 {
                out.push(' ');
            }
        }

        out.push_str(" |");
        // ASCII representation
        for &b in chunk {
            if b.is_ascii_graphic() || b == b' ' {
                out.push(b as char);
            } else {
                out.push('.');
            }
        }
        out.push_str("|\n");
    }

    out
}

/// Format bytes as printable text, escaping non-printable characters
pub fn format_text(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    for &b in data {
        if b == b'\n' || b == b'\r' || b == b'\t' {
            out.push(b as char);
        } else if b.is_ascii_graphic() || b == b' ' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{:02x}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hexdump() {
        let data = b"Hello, World!\x00\xff";
        let dump = hexdump(data);
        assert!(dump.contains("00000000"));
        assert!(dump.contains("48 65 6c 6c 6f"));
        assert!(dump.contains("|Hello, World!..|"));
    }

    #[test]
    fn test_format_text() {
        let data = b"Hello\nWorld\x01";
        assert_eq!(format_text(data), "Hello\nWorld\\x01");
    }
}
