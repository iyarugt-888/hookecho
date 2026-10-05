//! Just enough of the zip format to read the archives GIS files arrive in: a KMZ's KML and a
//! zipped shapefile's `.shp`/`.dbf`/`.prj`/`.cpg`. Stored and deflated entries only, every offset
//! and length checked against the archive, and inflation capped by the caller so a hostile
//! archive cannot drive an unbounded allocation. Zip64, encryption and other compression methods
//! are refused by name rather than misread.

use std::io::Read;

/// One file in the archive's central directory.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The path inside the archive, as written (`/`-separated).
    pub name: String,
    flags: u16,
    method: u16,
    local: usize,
    comp: u64,
    /// The size the archive declares for the inflated file. The archive's word, not a fact:
    /// [`read`] enforces its own limit while inflating.
    pub size: u64,
}

impl Entry {
    /// A directory record rather than a file.
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

fn u16_at(b: &[u8], at: usize) -> anyhow::Result<u16> {
    let s = b
        .get(at..at + 2)
        .ok_or_else(|| anyhow::anyhow!("the zip is cut short"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> anyhow::Result<u32> {
    let s = b
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("the zip is cut short"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Is this file a zip archive, judging by its first bytes rather than its name?
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Every entry in the archive's central directory, in archive order.
pub fn entries(zip: &[u8]) -> anyhow::Result<Vec<Entry>> {
    // The end-of-central-directory record: 22 bytes plus a comment of up to 64 KiB, at the end.
    let floor = zip.len().saturating_sub(22 + 0xFFFF);
    let eocd = (floor..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or_else(|| anyhow::anyhow!("not a zip archive (no central directory)"))?;
    let count = u16_at(zip, eocd + 10)? as usize;
    let mut at = u32_at(zip, eocd + 16)? as usize;
    anyhow::ensure!(
        at != 0xFFFF_FFFF,
        "a zip64 archive is not supported; re-save it as an ordinary zip"
    );
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        anyhow::ensure!(
            zip.get(at..at + 4) == Some(b"PK\x01\x02"),
            "a damaged zip central directory"
        );
        let flags = u16_at(zip, at + 8)?;
        let method = u16_at(zip, at + 10)?;
        let comp = u32_at(zip, at + 20)? as u64;
        let size = u32_at(zip, at + 24)? as u64;
        let name_len = u16_at(zip, at + 28)? as usize;
        let extra = u16_at(zip, at + 30)? as usize;
        let comment = u16_at(zip, at + 32)? as usize;
        let local = u32_at(zip, at + 42)? as usize;
        let name = zip
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| anyhow::anyhow!("the zip is cut short"))?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            flags,
            method,
            local,
            comp,
            size,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// One entry's content, refused when it would inflate past `max` bytes.
pub fn read(zip: &[u8], e: &Entry, max: u64) -> anyhow::Result<Vec<u8>> {
    let name = &e.name;
    anyhow::ensure!(e.flags & 1 == 0, "{name} is encrypted");
    anyhow::ensure!(e.size <= max, "{name} is too large ({} bytes)", e.size);
    let local = e.local;
    anyhow::ensure!(
        zip.get(local..local + 4) == Some(b"PK\x03\x04"),
        "a damaged zip entry for {name}"
    );
    let start = local + 30 + u16_at(zip, local + 26)? as usize + u16_at(zip, local + 28)? as usize;
    let data = zip
        .get(start..start.saturating_add(e.comp as usize))
        .ok_or_else(|| anyhow::anyhow!("{name} runs past the end of the zip"))?;
    match e.method {
        // Stored: the bytes are what they are, whatever size the directory declared.
        0 => {
            anyhow::ensure!(
                data.len() as u64 <= max,
                "{name} holds {} bytes, past {max}",
                data.len()
            );
            Ok(data.to_vec())
        }
        8 => {
            // Grown as it inflates: the declared size is the archive's word, not a fact.
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(data)
                .take(max + 1)
                .read_to_end(&mut out)?;
            anyhow::ensure!(out.len() as u64 <= max, "{name} inflates past {max} bytes");
            Ok(out)
        }
        m => anyhow::bail!("{name} uses zip compression method {m}, which is not supported"),
    }
}

/// A small zip writer for tests in this crate: stored or deflated entries, no extras.
#[cfg(test)]
pub(crate) fn build(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in entries {
        let body = if *deflate {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(data).unwrap();
            enc.finish().unwrap()
        } else {
            data.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let local = out.len() as u32;
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 8]); // time, date, crc
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);

        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 12]); // extra, comment, disk, internal and external attrs
        central.extend_from_slice(&local.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_at = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_and_deflated_entries_read_back() {
        let z = build(&[
            ("a.txt", b"hello", false),
            ("d/b.txt", b"world world", true),
        ]);
        assert!(is_zip(&z));
        let es = entries(&z).unwrap();
        assert_eq!(es.len(), 2);
        assert_eq!(es[1].name, "d/b.txt");
        assert_eq!(read(&z, &es[0], 100).unwrap(), b"hello");
        assert_eq!(read(&z, &es[1], 100).unwrap(), b"world world");
    }

    #[test]
    fn an_entry_past_the_limit_is_refused_even_when_it_lies_about_its_size() {
        let big = vec![b'x'; 10_000];
        let mut z = build(&[("big", &big, true)]);
        // Rewrite the central directory's declared size to something small.
        let cd = z.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        z[cd + 24..cd + 28].copy_from_slice(&10u32.to_le_bytes());
        let es = entries(&z).unwrap();
        let err = read(&z, &es[0], 1000).unwrap_err().to_string();
        assert!(err.contains("inflates past"), "{err}");
    }

    #[test]
    fn a_stored_entry_past_the_limit_is_refused_even_when_it_lies_about_its_size() {
        let big = vec![b'x'; 10_000];
        let mut z = build(&[("big", &big, false)]);
        let cd = z.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        z[cd + 24..cd + 28].copy_from_slice(&10u32.to_le_bytes());
        let es = entries(&z).unwrap();
        let err = read(&z, &es[0], 1000).unwrap_err().to_string();
        assert!(err.contains("past 1000"), "{err}");
    }

    #[test]
    fn no_truncation_of_an_archive_can_panic() {
        let z = build(&[("a.shp", b"0123456789", true), ("a.dbf", b"abc", false)]);
        for n in 0..z.len() {
            if let Ok(es) = entries(&z[..n]) {
                for e in &es {
                    let _ = read(&z[..n], e, 1 << 20);
                }
            }
        }
    }
}
