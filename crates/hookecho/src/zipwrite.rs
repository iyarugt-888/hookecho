//! A minimal ZIP writer: deflate-compressed entries, a central directory, nothing else — enough to
//! hand an analysis export (ROADMAP_NEW K4) to any platform as one file (a save dialog on desktop,
//! a download in a browser, a share on Android) without a new dependency. `flate2`, already here
//! for other reasons, supplies both the raw deflate stream and the CRC-32 the format needs.
//!
//! Not written: ZIP64 (every export is far under 4 GB and 65 535 entries), encryption, extra
//! fields, or comments. Names are stored as given and flagged UTF-8.

use chrono::{DateTime, Datelike, Timelike, Utc};
use std::io::Write;

/// The MS-DOS time and date fields a ZIP header stores, from a UTC instant (the format has no time
/// zone; UTC is what an analysis export means anyway). Years before 1980 cannot be represented and
/// clamp to it.
fn dos_time_date(t: DateTime<Utc>) -> (u16, u16) {
    let time = ((t.hour() as u16) << 11) | ((t.minute() as u16) << 5) | (t.second() as u16 / 2);
    let year = (t.year().clamp(1980, 2107) - 1980) as u16;
    let date = (year << 9) | ((t.month() as u16) << 5) | t.day() as u16;
    (time, date)
}

/// The bytes of a ZIP archive holding `entries` (name, contents), each deflated, stamped `when`.
pub fn zip(entries: &[(String, Vec<u8>)], when: DateTime<Utc>) -> Vec<u8> {
    let (time, date) = dos_time_date(when);
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    // Bit 11: file names are UTF-8.
    const FLAGS: u16 = 1 << 11;
    const DEFLATE: u16 = 8;
    const VERSION: u16 = 20;
    for (name, data) in entries {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let crc = crc.sum();
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        // Writing to a Vec cannot fail.
        enc.write_all(data).expect("deflate into memory");
        let packed = enc.finish().expect("deflate into memory");
        let offset = out.len() as u32;
        let name = name.as_bytes();

        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        for v in [VERSION, FLAGS, DEFLATE, time, date] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, packed.len() as u32, data.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        out.extend_from_slice(name);
        out.extend_from_slice(&packed);

        // Its central-directory record.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        for v in [VERSION, VERSION, FLAGS, DEFLATE, time, date] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, packed.len() as u32, data.len() as u32] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        // Name length, extra length, comment length, disk number, internal attributes.
        for v in [name.len() as u16, 0, 0, 0, 0] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let dir_offset = out.len() as u32;
    let dir_len = central.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    let n = entries.len() as u16;
    for v in [0u16, 0, n, n] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&dir_len.to_le_bytes());
    out.extend_from_slice(&dir_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Read the archive back the way an unzip tool does: from the end-of-directory record, through
    /// the central directory, to each local entry, inflating and CRC-checking it.
    fn unzip(z: &[u8]) -> Vec<(String, Vec<u8>)> {
        let eocd = z.len() - 22;
        assert_eq!(u32_at(z, eocd), 0x0605_4b50);
        let n = u16_at(z, eocd + 10) as usize;
        let mut at = u32_at(z, eocd + 16) as usize;
        let mut out = Vec::new();
        for _ in 0..n {
            assert_eq!(u32_at(z, at), 0x0201_4b50);
            let crc = u32_at(z, at + 16);
            let packed = u32_at(z, at + 20) as usize;
            let size = u32_at(z, at + 24) as usize;
            let name_len = u16_at(z, at + 28) as usize;
            let local = u32_at(z, at + 42) as usize;
            let name = String::from_utf8(z[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(u32_at(z, local), 0x0403_4b50);
            let data_at = local + 30 + u16_at(z, local + 26) as usize;
            let mut data = Vec::new();
            flate2::read::DeflateDecoder::new(&z[data_at..data_at + packed])
                .read_to_end(&mut data)
                .unwrap();
            assert_eq!(data.len(), size);
            let mut c = flate2::Crc::new();
            c.update(&data);
            assert_eq!(c.sum(), crc, "{name}");
            out.push((name, data));
            at += 46 + name_len;
        }
        out
    }

    #[test]
    fn an_archive_reads_back_entry_for_entry() {
        let entries = vec![
            ("README.txt".to_string(), b"hello".to_vec()),
            (
                "probes/region.csv".to_string(),
                "a,b\n1,2\n".repeat(500).into_bytes(),
            ),
            ("empty.json".to_string(), Vec::new()),
            ("ümlaut.txt".to_string(), b"utf-8 name".to_vec()),
        ];
        let when = "2013-05-20T20:08:31Z".parse().unwrap();
        let z = zip(&entries, when);
        assert_eq!(unzip(&z), entries);
        // Deflate actually compresses the repetitive one.
        assert!(z.len() < 4000 + 5 + 20 + 1000, "{}", z.len());
        assert!(unzip(&zip(&[], when)).is_empty());
    }

    #[test]
    fn the_dos_timestamp_packs_fields_in_place() {
        let (time, date) = dos_time_date("2013-05-20T20:08:31Z".parse().unwrap());
        assert_eq!(time >> 11, 20);
        assert_eq!((time >> 5) & 0x3f, 8);
        assert_eq!(time & 0x1f, 15, "two-second resolution");
        assert_eq!(date >> 9, 2013 - 1980);
        assert_eq!((date >> 5) & 0xf, 5);
        assert_eq!(date & 0x1f, 20);
        let (_, old) = dos_time_date("1970-01-01T00:00:00Z".parse().unwrap());
        assert_eq!(old >> 9, 0, "before 1980 clamps to it");
    }
}
