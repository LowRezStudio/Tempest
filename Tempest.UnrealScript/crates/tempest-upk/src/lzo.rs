//! LZO1X decompressor (the `lzo1x_decompress_safe` format used by
//! `appUncompressMemory` with `COMPRESS_LZO`) and a simple LZO1X-1 style
//! compressor good enough to produce streams the engine accepts.

use crate::io::{Error, Result};

const M2_MAX_OFFSET: usize = 0x0800;

fn fail(msg: &str) -> Error {
    Error::Compression(msg.to_string())
}

/// Decompress `input` into a buffer of exactly `out_len` bytes.
pub fn decompress(input: &[u8], out_len: usize) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    let mut ip = 0usize;

    macro_rules! need {
        ($n:expr) => {
            if ip + $n > input.len() {
                return Err(fail("input overrun"));
            }
        };
    }
    macro_rules! byte {
        () => {{
            need!(1);
            let b = input[ip];
            ip += 1;
            b as usize
        }};
    }
    macro_rules! copy_literals {
        ($n:expr) => {{
            let n = $n;
            need!(n);
            if out.len() + n > out_len {
                return Err(fail("output overrun (literals)"));
            }
            out.extend_from_slice(&input[ip..ip + n]);
            ip += n;
        }};
    }
    macro_rules! copy_match {
        ($dist:expr, $n:expr) => {{
            let dist = $dist;
            let n = $n;
            if dist == 0 || dist > out.len() {
                return Err(fail("lookbehind overrun"));
            }
            if out.len() + n > out_len {
                return Err(fail("output overrun (match)"));
            }
            let start = out.len() - dist;
            for k in 0..n {
                let b = out[start + k];
                out.push(b);
            }
        }};
    }

    let mut t: usize;
    let mut state: u8; // 0 = literal run, 1 = first literal run, 2 = match, 3 = match_next
    if input.is_empty() {
        return Err(fail("empty input"));
    }
    if input[0] > 17 {
        t = byte!() - 17;
        if t < 4 {
            state = 3;
        } else {
            copy_literals!(t);
            state = 1;
        }
    } else {
        t = 0;
        state = 0;
    }

    loop {
        match state {
            0 => {
                t = byte!();
                if t >= 16 {
                    state = 2;
                    continue;
                }
                if t == 0 {
                    loop {
                        need!(1);
                        if input[ip] != 0 {
                            break;
                        }
                        t += 255;
                        ip += 1;
                    }
                    t += 15 + byte!();
                }
                copy_literals!(t + 3);
                state = 1;
            }
            1 => {
                t = byte!();
                if t >= 16 {
                    state = 2;
                    continue;
                }
                let dist = 1 + M2_MAX_OFFSET + (t >> 2) + (byte!() << 2);
                copy_match!(dist, 3);
                // match_done
                t = input[ip - 2] as usize & 3;
                state = if t == 0 { 0 } else { 3 };
            }
            2 => {
                if t >= 64 {
                    let dist = 1 + ((t >> 2) & 7) + (byte!() << 3);
                    let n = (t >> 5) + 1;
                    copy_match!(dist, n);
                } else if t >= 32 {
                    t &= 31;
                    if t == 0 {
                        loop {
                            need!(1);
                            if input[ip] != 0 {
                                break;
                            }
                            t += 255;
                            ip += 1;
                        }
                        t += 31 + byte!();
                    }
                    need!(2);
                    let dist = 1 + ((input[ip] as usize) >> 2) + ((input[ip + 1] as usize) << 6);
                    ip += 2;
                    copy_match!(dist, t + 2);
                } else if t >= 16 {
                    let mut dist = (t & 8) << 11;
                    t &= 7;
                    if t == 0 {
                        loop {
                            need!(1);
                            if input[ip] != 0 {
                                break;
                            }
                            t += 255;
                            ip += 1;
                        }
                        t += 7 + byte!();
                    }
                    need!(2);
                    dist += ((input[ip] as usize) >> 2) + ((input[ip + 1] as usize) << 6);
                    ip += 2;
                    if dist == 0 {
                        // EOF marker
                        if out.len() != out_len {
                            return Err(fail(&format!(
                                "size mismatch: expected {out_len}, got {}",
                                out.len()
                            )));
                        }
                        return Ok(out);
                    }
                    dist += 0x4000;
                    copy_match!(dist, t + 2);
                } else {
                    let dist = 1 + (t >> 2) + (byte!() << 2);
                    copy_match!(dist, 2);
                }
                // match_done
                t = input[ip - 2] as usize & 3;
                state = if t == 0 { 0 } else { 3 };
            }
            _ => {
                // match_next: 1..3 literals then another match
                copy_literals!(t);
                t = byte!();
                state = 2;
            }
        }
    }
}

/// Minimal LZO1X-1 compatible compressor (greedy hash matching).
///
/// Output is a valid LZO1X stream decodable by `lzo1x_decompress_safe`. It
/// is not byte-identical to what the Epic/Hi-Rez cooker produced, but the
/// engine only cares that it decodes.
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2 + 16);
    let n = input.len();
    if n == 0 {
        out.extend_from_slice(&[0x11, 0, 0]);
        return out;
    }

    const HASH_BITS: usize = 14;
    let mut table = vec![usize::MAX; 1 << HASH_BITS];
    let hash = |p: &[u8]| -> usize {
        let v = u32::from_le_bytes([p[0], p[1], p[2], 0]).wrapping_mul(0x9E37_79B1);
        (v >> (32 - HASH_BITS)) as usize
    };

    let mut lit_start = 0usize;
    let mut ip = 0usize;
    let mut first_literal = true;
    let mut last_match_lit_trailer: Option<usize> = None; // position of byte holding the 2-bit literal count

    let flush_literals = |out: &mut Vec<u8>,
                          input: &[u8],
                          lit_start: usize,
                          lit_end: usize,
                          first_literal: &mut bool,
                          trailer: &mut Option<usize>| {
        let len = lit_end - lit_start;
        if len == 0 {
            return;
        }
        if *first_literal {
            *first_literal = false;
            if len <= 238 {
                out.push((len + 17) as u8);
            } else {
                // first literal must use the long form via the regular encoding
                out.push(0);
                let mut rem = len - 3 - 15;
                while rem > 255 {
                    out.push(0);
                    rem -= 255;
                }
                out.push(rem as u8);
            }
            out.extend_from_slice(&input[lit_start..lit_end]);
            *trailer = None;
            return;
        }
        if len <= 3 {
            if let Some(pos) = *trailer {
                out[pos] |= len as u8;
                out.extend_from_slice(&input[lit_start..lit_end]);
                *trailer = None;
                return;
            }
        }
        if len <= 18 {
            out.push((len - 3) as u8);
        } else {
            out.push(0);
            let mut rem = len - 3 - 15;
            while rem > 255 {
                out.push(0);
                rem -= 255;
            }
            out.push(rem as u8);
        }
        out.extend_from_slice(&input[lit_start..lit_end]);
        *trailer = None;
    };

    while ip + 3 <= n {
        let h = hash(&input[ip..ip + 3]);
        let cand = table[h];
        table[h] = ip;
        if cand != usize::MAX && ip - cand <= 0xBFFF && input[cand..cand + 3] == input[ip..ip + 3] {
            // extend match
            let mut len = 3;
            while ip + len < n && len < 0x1000 && input[cand + len] == input[ip + len] {
                len += 1;
            }
            let dist = ip - cand;
            // literals preceding the match
            flush_literals(
                &mut out,
                input,
                lit_start,
                ip,
                &mut first_literal,
                &mut last_match_lit_trailer,
            );
            // emit match
            if len <= 8 && dist <= M2_MAX_OFFSET {
                let d = dist - 1;
                out.push((((len - 1) << 5) | ((d & 7) << 2)) as u8);
                out.push((d >> 3) as u8);
                last_match_lit_trailer = Some(out.len() - 2);
            } else if dist <= 0x4000 {
                let d = dist - 1;
                let mut l = len - 2;
                if l <= 31 {
                    out.push((32 | l) as u8);
                } else {
                    out.push(32);
                    l -= 31;
                    while l > 255 {
                        out.push(0);
                        l -= 255;
                    }
                    out.push(l as u8);
                }
                out.push(((d & 63) << 2) as u8);
                out.push((d >> 6) as u8);
                last_match_lit_trailer = Some(out.len() - 2);
            } else {
                let d = dist - 0x4000;
                let mut l = len - 2;
                let hi = ((d >> 11) & 8) as u8;
                if l <= 7 {
                    out.push(16 | hi | l as u8);
                } else {
                    out.push(16 | hi);
                    l -= 7;
                    while l > 255 {
                        out.push(0);
                        l -= 255;
                    }
                    out.push(l as u8);
                }
                out.push(((d & 63) << 2) as u8);
                out.push((d >> 6) as u8);
                last_match_lit_trailer = Some(out.len() - 2);
            }
            // insert hashes for the matched region
            let mut k = ip + 1;
            while k + 3 <= ip + len && k + 3 <= n {
                let hh = hash(&input[k..k + 3]);
                table[hh] = k;
                k += 1;
            }
            ip += len;
            lit_start = ip;
        } else {
            ip += 1;
        }
    }
    flush_literals(&mut out, input, lit_start, n, &mut first_literal, &mut last_match_lit_trailer);
    // EOF marker
    out.extend_from_slice(&[0x11, 0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small() {
        let data = b"hello hello hello hello world world world ABCABCABCABCABC".to_vec();
        let c = compress(&data);
        let d = decompress(&c, data.len()).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn roundtrip_random_like() {
        let mut data = Vec::new();
        let mut x: u32 = 12345;
        for i in 0..70000 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            data.push(if i % 7 == 0 { (x >> 16) as u8 } else { (i % 13) as u8 });
        }
        let c = compress(&data);
        let d = decompress(&c, data.len()).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn roundtrip_long_literals() {
        let mut data = Vec::new();
        let mut x: u32 = 99;
        for _ in 0..5000 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            data.push((x >> 24) as u8);
        }
        let c = compress(&data);
        let d = decompress(&c, data.len()).unwrap();
        assert_eq!(d, data);
    }
}
