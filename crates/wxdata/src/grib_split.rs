//! Pulling one field out of a GRIB2 message that carries several.
//!
//! GRIB2 lets a message repeat its product, representation, bitmap and data sections (4–7), and
//! sometimes its grid (3), to carry more than one field under one indicator: RAP's `awp130pgrb`
//! and the NAM nest pack `UGRD` and `VGRD` for a level that way, and their `.idx` lists the two as
//! `92.1` and `92.2` at one shared byte offset. `gribberish` decodes the first field of a message
//! only, so asking for the second quietly returns the first (a V wind that is really U — see
//! `hrrr::fetch_wind`'s warning).
//!
//! [`extract_field`] rebuilds the `k`-th field as a standalone single-field message — the
//! indicator, the identification and the most recent local-use and grid sections, then that
//! field's own sections 4–7 and the end marker — which any GRIB2 decoder, `gribberish` included,
//! reads as the ordinary thing it is. A bitmap section that says "use the previously defined
//! bitmap" (indicator 254) is replaced by that earlier bitmap, so the rebuilt message stands alone.

/// The `k`-th (0-based) field of the GRIB2 message at the start of `raw`, as a message of its
/// own; `None` when `raw` is not GRIB2, is cut short, or has fewer than `k + 1` fields.
pub fn extract_field(raw: &[u8], k: usize) -> Option<Vec<u8>> {
    if raw.get(..4)? != b"GRIB" || *raw.get(7)? != 2 {
        return None;
    }
    let total = u64::from_be_bytes(raw.get(8..16)?.try_into().ok()?) as usize;
    let msg = raw.get(..total.min(raw.len()))?;
    let mut latest: [Option<&[u8]>; 8] = [None; 8];
    let mut real_bitmap: Option<&[u8]> = None;
    let mut field = 0;
    let mut at = 16;
    while at + 4 <= msg.len() {
        if &msg[at..at + 4] == b"7777" {
            break;
        }
        let len = u32::from_be_bytes(msg.get(at..at + 4)?.try_into().ok()?) as usize;
        let num = *msg.get(at + 4)? as usize;
        if len < 5 || num > 7 {
            return None;
        }
        let section = msg.get(at..at + len)?;
        if num == 6 {
            match section.get(5) {
                Some(0) => real_bitmap = Some(section),
                Some(254) => {
                    // "The bitmap defined earlier in this message applies": carry it over.
                    latest[6] = real_bitmap;
                    at += len;
                    continue;
                }
                _ => {}
            }
        }
        latest[num] = Some(section);
        if num == 7 {
            if field == k {
                let mut out = Vec::with_capacity(total);
                out.extend_from_slice(&msg[..16]);
                for (n, section) in latest.iter().enumerate().skip(1) {
                    if let Some(s) = section {
                        out.extend_from_slice(s);
                    } else if n != 2 {
                        return None; // every section but local use is required
                    }
                }
                out.extend_from_slice(b"7777");
                let len = out.len() as u64;
                out[8..16].copy_from_slice(&len.to_be_bytes());
                return Some(out);
            }
            field += 1;
        }
        at += len;
    }
    None
}

/// Which field of its message an `.idx` line describes: `92.2` is the second (index 1), a plain
/// `92` the first.
pub fn subfield_of(idx_number: &str) -> usize {
    idx_number
        .split_once('.')
        .and_then(|(_, sub)| sub.parse::<usize>().ok())
        .map_or(0, |s| s.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(num: u8, body: &[u8]) -> Vec<u8> {
        let mut s = ((body.len() + 5) as u32).to_be_bytes().to_vec();
        s.push(num);
        s.extend_from_slice(body);
        s
    }

    /// A two-field message: shared 1 and 3, then 4-7 twice; the second bitmap is "as before".
    fn two_fields() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(section(1, b"ident"));
        body.extend(section(3, b"grid"));
        body.extend(section(4, b"prod-U"));
        body.extend(section(5, b"repr-U"));
        body.extend(section(6, &[0, 0xAA]));
        body.extend(section(7, b"data-U"));
        body.extend(section(4, b"prod-V"));
        body.extend(section(5, b"repr-V"));
        body.extend(section(6, &[254]));
        body.extend(section(7, b"data-V"));
        let mut m = b"GRIB\0\0\0\x02".to_vec();
        m.extend(((16 + body.len() + 4) as u64).to_be_bytes());
        m.extend(body);
        m.extend(b"7777");
        m
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn the_second_field_stands_alone_with_the_shared_sections_and_the_bitmap() {
        let m = two_fields();
        let v = extract_field(&m, 1).unwrap();
        assert!(v.starts_with(b"GRIB") && v.ends_with(b"7777"));
        assert_eq!(
            u64::from_be_bytes(v[8..16].try_into().unwrap()) as usize,
            v.len()
        );
        for part in [
            &b"ident"[..],
            b"grid",
            b"prod-V",
            b"repr-V",
            b"data-V",
            &[0, 0xAA],
        ] {
            assert!(contains(&v, part), "missing {part:?}");
        }
        assert!(!contains(&v, b"data-U") && !contains(&v, &[254]));
        let u = extract_field(&m, 0).unwrap();
        assert!(contains(&u, b"data-U") && !contains(&u, b"data-V"));
        assert!(extract_field(&m, 2).is_none());
    }

    #[test]
    fn broken_input_is_none_never_a_panic() {
        let m = two_fields();
        for cut in 0..m.len() {
            let _ = extract_field(&m[..cut], 1);
        }
        assert!(extract_field(b"not grib at all", 0).is_none());
        assert_eq!(subfield_of("92.2"), 1);
        assert_eq!(subfield_of("92"), 0);
        assert_eq!(subfield_of("92.1"), 0);
    }
}
