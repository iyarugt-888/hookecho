//! Loop and still export (ROADMAP_NEW M2). Animated GIF and MP4 of a radar loop with each frame
//! held for its own time — the real gap between scans, or a fixed rate — and single images as
//! PNG, JPEG or WebP. The app captures loop frames via the screenshot path (GUI-only) and
//! `--watch` renders them off screen at a fixed size; both end up here. The encoders and the
//! timing are the testable part; the capture state machines live in [`crate::app`] and
//! [`crate::watch`].

// The GIF encoder is native-only, and so is the codec behind it: an export needs a file to write,
// `paths::data_dir()` is `None` in a browser, and so the web build's export path already returned
// before it ever reached here. Compiling the codec in anyway was pure bundle weight.
#[cfg(not(target_arch = "wasm32"))]
use image::codecs::gif::{GifEncoder, Repeat};
use image::RgbaImage;
#[cfg(not(target_arch = "wasm32"))]
use image::{Delay, Frame};
use std::path::Path;

/// Output container for a loop export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopFormat {
    Gif,
    Mp4,
}

/// How long each loop frame is held.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Timing {
    /// Every frame for `1/fps` seconds.
    Fixed { fps: f32 },
    /// Each frame for the real time until the next scan, scaled so the loop still averages `fps`
    /// frames a second: a 2-minute SAILS gap stays half as long as a 4-minute one, and a gap in
    /// the data reads as a pause rather than a jump.
    Real { fps: f32 },
}

/// The shortest and longest a frame is held, ms. The floor is two GIF ticks (browsers stretch
/// anything shorter to 100 ms); the ceiling keeps an outage in the data from stalling the loop.
const MIN_DELAY_MS: u32 = 20;
const MAX_DELAY_MS: u32 = 4_000;
/// The last frame dwells this many times its own delay, so the loop's newest picture registers
/// before it starts over — the convention every radar loop follows.
const LAST_FRAME_DWELL: u32 = 3;

/// Each frame's hold, ms, for frames valid at `times` (oldest first).
pub fn frame_delays_ms(times: &[chrono::DateTime<chrono::Utc>], timing: Timing) -> Vec<u32> {
    let n = times.len();
    if n == 0 {
        return Vec::new();
    }
    let (fps, real) = match timing {
        Timing::Fixed { fps } => (fps, false),
        Timing::Real { fps } => (fps, true),
    };
    let base = 1000.0 / fps.clamp(0.1, 60.0);
    let gaps: Vec<f64> = times
        .windows(2)
        .map(|w| (w[1] - w[0]).num_milliseconds().max(0) as f64)
        .collect();
    let mean = if gaps.is_empty() {
        0.0
    } else {
        gaps.iter().sum::<f64>() / gaps.len() as f64
    };
    let mut out: Vec<u32> = (0..n)
        .map(|i| {
            let ms = match gaps.get(i) {
                Some(g) if real && mean > 0.0 => f64::from(base) * g / mean,
                _ => f64::from(base),
            };
            (ms.round() as u32).clamp(MIN_DELAY_MS, MAX_DELAY_MS)
        })
        .collect();
    // The last frame has no next scan to time against: it takes the one before it, then dwells.
    if n > 1 {
        out[n - 1] = out[n - 2];
    }
    if let Some(last) = out.last_mut() {
        *last = (*last * LAST_FRAME_DWELL).min(MAX_DELAY_MS);
    }
    out
}

/// One frame of a loop, as its sidecar lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LoopFrame {
    pub volume: String,
    pub valid_time_utc: String,
    /// How long the frame is held, ms.
    pub delay_ms: u32,
    /// When it appears, ms from the start of the loop.
    pub start_ms: u64,
}

/// A loop's frame list: each volume, its valid time, its hold and when it starts — what both the
/// app's and `--watch`'s loop sidecars carry.
pub fn frame_list(
    volumes: &[(String, chrono::DateTime<chrono::Utc>)],
    timing: Timing,
) -> Vec<LoopFrame> {
    let times: Vec<_> = volumes.iter().map(|(_, t)| *t).collect();
    let mut start = 0u64;
    volumes
        .iter()
        .zip(frame_delays_ms(&times, timing))
        .map(|((name, t), delay_ms)| {
            let f = LoopFrame {
                volume: name.clone(),
                valid_time_utc: t.to_rfc3339(),
                delay_ms,
                start_ms: start,
            };
            start += u64::from(delay_ms);
            f
        })
        .collect()
}

/// The words a loop's sidecar uses for its timing: `("real" | "fixed", fps)`.
pub fn timing_words(timing: Timing) -> (&'static str, f32) {
    match timing {
        Timing::Real { fps } => ("real", fps),
        Timing::Fixed { fps } => ("fixed", fps),
    }
}

/// Encode `frames` into an MP4 (H.264) at `path` via the `ffmpeg` CLI, every frame for `1/fps`.
pub fn encode_mp4(frames: &[RgbaImage], fps: u32, path: &Path) -> anyhow::Result<()> {
    let delay = 1000 / fps.max(1);
    encode_mp4_timed(frames, &vec![delay; frames.len()], path)
}

/// Encode `frames` into an MP4 at `path`, each held for its `delays_ms`. Frames are staged as PNGs
/// and handed to ffmpeg's concat demuxer with a duration each, then written at a constant 30 fps
/// (repeating frames to fill each hold), which every player plays; a variable-rate file does not.
pub fn encode_mp4_timed(
    frames: &[RgbaImage],
    delays_ms: &[u32],
    path: &Path,
) -> anyhow::Result<()> {
    if frames.is_empty() {
        anyhow::bail!("no frames captured");
    }
    anyhow::ensure!(frames.len() == delays_ms.len(), "one delay per frame");
    // Stage PNGs in a unique temp dir.
    let dir = std::env::temp_dir().join(format!(
        "hookecho_mp4_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir)?;
    let result = (|| {
        let mut files = Vec::with_capacity(frames.len());
        for (i, img) in frames.iter().enumerate() {
            let file = dir.join(format!("f{i:05}.png"));
            img.save(&file)?;
            files.push(file);
        }
        encode_mp4_files(&files, delays_ms, path)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Encode already-written frame images (`files`, in order) into an MP4 at `path`, each held for
/// its `delays_ms` — the off-screen path, whose frames are on disk and need not all be in memory.
pub fn encode_mp4_files(
    files: &[std::path::PathBuf],
    delays_ms: &[u32],
    path: &Path,
) -> anyhow::Result<()> {
    anyhow::ensure!(!files.is_empty(), "no frames captured");
    anyhow::ensure!(files.len() == delays_ms.len(), "one delay per frame");
    let mut list = String::new();
    for (file, ms) in files.iter().zip(delays_ms) {
        list.push_str(&concat_entry(&concat_path(file), *ms));
    }
    // The concat demuxer ignores the last entry's duration unless the file is listed again.
    if let Some(last) = files.last() {
        list.push_str(&format!("file '{}'\n", concat_path(last)));
    }
    let list_path = path.with_extension("frames.txt");
    std::fs::write(&list_path, list)?;
    let result = run_ffmpeg(&list_path, path);
    let _ = std::fs::remove_file(&list_path);
    result
}

/// A path as the concat list quotes it: forward slashes, and `'` escaped the way ffmpeg reads it.
fn concat_path(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "/")
        .replace('\'', r"'\''")
}

/// One concat-demuxer entry: the file and how long it is shown, in seconds.
fn concat_entry(name: &str, ms: u32) -> String {
    format!("file '{name}'\nduration {:.3}\n", f64::from(ms) / 1000.0)
}

fn run_ffmpeg(list: &Path, out: &Path) -> anyhow::Result<()> {
    let mut ffmpeg = std::process::Command::new("ffmpeg");
    crate::platform::no_window(&mut ffmpeg);
    let status = ffmpeg
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(list)
        .args([
            // Even dimensions are required by yuv420p; pad if odd.
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "-r",
            "30",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => anyhow::bail!("ffmpeg exited {s}"),
        Err(e) => anyhow::bail!("ffmpeg not available ({e}); GIF export works without it"),
    }
}

/// Encode `frames` into a looping GIF at `path`, each shown for `delay_ms` milliseconds.
#[cfg(not(target_arch = "wasm32"))]
pub fn encode_gif(frames: &[RgbaImage], delay_ms: u16, path: &Path) -> anyhow::Result<()> {
    encode_gif_timed(
        frames.iter().cloned().map(Ok),
        &vec![u32::from(delay_ms); frames.len()],
        path,
    )
}

/// Encode frames into a looping GIF at `path`, each held for its `delays_ms`. Frames arrive one at
/// a time, so a long 4K loop read back from disk never has to fit in memory at once.
#[cfg(not(target_arch = "wasm32"))]
pub fn encode_gif_timed(
    frames: impl IntoIterator<Item = anyhow::Result<RgbaImage>>,
    delays_ms: &[u32],
    path: &Path,
) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = GifEncoder::new(std::io::BufWriter::new(file));
    enc.set_repeat(Repeat::Infinite)?;
    let mut n = 0;
    for (img, ms) in frames.into_iter().zip(delays_ms) {
        let delay = Delay::from_numer_denom_ms(*ms, 1);
        enc.encode_frame(Frame::from_parts(img?, 0, 0, delay))?;
        n += 1;
    }
    drop(enc);
    if n == 0 {
        let _ = std::fs::remove_file(path);
        anyhow::bail!("no frames captured");
    }
    Ok(())
}

/// A still image format, chosen by the output's file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StillFormat {
    Png,
    Jpeg,
    Webp,
}

impl StillFormat {
    /// The format a path's extension names, if it names a still one.
    pub fn from_path(path: &Path) -> Option<StillFormat> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(StillFormat::Png),
            "jpg" | "jpeg" => Some(StillFormat::Jpeg),
            "webp" => Some(StillFormat::Webp),
            _ => None,
        }
    }
}

/// Encode one still. JPEG is written at quality 90 and has no alpha, so the picture is flattened
/// onto black first (the map's own background); WebP is lossless.
pub fn encode_still(img: &RgbaImage, format: StillFormat) -> anyhow::Result<Vec<u8>> {
    use image::ImageEncoder as _;
    let mut out = Vec::new();
    match format {
        StillFormat::Png => image::codecs::png::PngEncoder::new(&mut out).write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )?,
        StillFormat::Jpeg => {
            let rgb: Vec<u8> = img
                .pixels()
                .flat_map(|p| {
                    let a = u16::from(p[3]);
                    [0, 1, 2].map(|c| (u16::from(p[c]) * a / 255) as u8)
                })
                .collect();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).write_image(
                &rgb,
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        }
        #[cfg(target_arch = "wasm32")]
        StillFormat::Webp => anyhow::bail!("WebP is written by the desktop build only"),
        #[cfg(not(target_arch = "wasm32"))]
        StillFormat::Webp => image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )?,
    }
    Ok(out)
}

/// The centre `w`×`h` of `img` (clamped to what it has). The off-screen renderer draws squares;
/// a 16:9 or portrait frame is the middle of a square rendered at its longer edge, at the same
/// map scale.
pub fn crop_center(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.min(img.width()), h.min(img.height()));
    let x = (img.width() - w) / 2;
    let y = (img.height() - h) / 2;
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_multiframe_gif() {
        let mut a = RgbaImage::new(8, 8);
        let mut b = RgbaImage::new(8, 8);
        for p in a.pixels_mut() {
            *p = image::Rgba([255, 0, 0, 255]);
        }
        for p in b.pixels_mut() {
            *p = image::Rgba([0, 0, 255, 255]);
        }
        let dir = std::env::temp_dir().join("hookecho_gif_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.gif");
        encode_gif(&[a, b], 100, &path).expect("encode");
        let meta = std::fs::metadata(&path).expect("file written");
        assert!(meta.len() > 0, "gif is non-empty");
        // GIF magic header.
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..3], b"GIF");
    }

    fn t(h: u32, m: u32, s: u32) -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(2013, 5, 20, h, m, s).unwrap()
    }

    #[test]
    fn fixed_timing_holds_every_frame_alike_and_dwells_on_the_last() {
        let times = [t(20, 0, 0), t(20, 4, 0), t(20, 5, 0), t(20, 9, 0)];
        let d = frame_delays_ms(&times, Timing::Fixed { fps: 5.0 });
        assert_eq!(d, [200, 200, 200, 600]);
        assert!(frame_delays_ms(&[], Timing::Fixed { fps: 5.0 }).is_empty());
        assert_eq!(
            frame_delays_ms(&[t(20, 0, 0)], Timing::Real { fps: 5.0 }),
            [600]
        );
    }

    #[test]
    fn real_timing_keeps_the_scans_own_spacing_at_the_chosen_pace() {
        // Gaps of 4 min, 1 min (a SAILS rescan), 4 min: mean 3 min.
        let times = [t(20, 0, 0), t(20, 4, 0), t(20, 5, 0), t(20, 9, 0)];
        let d = frame_delays_ms(&times, Timing::Real { fps: 5.0 });
        assert_eq!(&d[..3], &[267, 67, 267]);
        // Real timing averages the requested pace over the gaps it times.
        let mean = d[..3].iter().sum::<u32>() as f32 / 3.0;
        assert!((mean - 200.0).abs() < 1.0, "{mean}");
        // A data outage does not freeze the loop, and nothing is shorter than a GIF can show.
        let outage = [t(20, 0, 0), t(20, 4, 0), t(23, 0, 0), t(23, 0, 1)];
        let d = frame_delays_ms(&outage, Timing::Real { fps: 5.0 });
        assert!(
            d.iter()
                .all(|ms| (MIN_DELAY_MS..=MAX_DELAY_MS).contains(ms)),
            "{d:?}"
        );
    }

    #[test]
    fn stills_encode_in_each_format_and_crop_to_the_middle() {
        let mut img = RgbaImage::new(40, 20);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba([x as u8 * 6, 80, 160, 255]);
        }
        let png = encode_still(&img, StillFormat::Png).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let jpg = encode_still(&img, StillFormat::Jpeg).unwrap();
        assert_eq!(&jpg[0..2], &[0xFF, 0xD8]);
        let webp = encode_still(&img, StillFormat::Webp).unwrap();
        assert_eq!(&webp[8..12], b"WEBP");
        let back = image::load_from_memory(&webp).unwrap().to_rgba8();
        assert_eq!(back, img, "WebP is lossless");
        let c = crop_center(&img, 20, 10);
        assert_eq!((c.width(), c.height()), (20, 10));
        assert_eq!(c.get_pixel(0, 0), img.get_pixel(10, 5));
        assert_eq!(
            StillFormat::from_path(Path::new("a/b.JPEG")),
            Some(StillFormat::Jpeg)
        );
        assert_eq!(StillFormat::from_path(Path::new("loop.gif")), None);
    }

    #[test]
    fn a_timed_gif_carries_each_frames_own_delay() {
        let frames: Vec<RgbaImage> = (0..3u8)
            .map(|i| RgbaImage::from_pixel(4, 4, image::Rgba([i * 80, 0, 0, 255])))
            .collect();
        let path = std::env::temp_dir().join("hookecho_gif_timed.gif");
        encode_gif_timed(frames.into_iter().map(Ok), &[100, 300, 900], &path).unwrap();
        use image::AnimationDecoder as _;
        let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let delays: Vec<u32> = image::codecs::gif::GifDecoder::new(file)
            .unwrap()
            .into_frames()
            .map(|f| {
                let (n, d) = f.unwrap().delay().numer_denom_ms();
                n / d
            })
            .collect();
        assert_eq!(delays, [100, 300, 900]);
    }

    #[test]
    fn the_mp4_frame_list_times_each_frame() {
        assert_eq!(
            concat_entry("f00000.png", 267),
            "file 'f00000.png'\nduration 0.267\n"
        );
        assert_eq!(
            concat_path(Path::new(r"C:\tmp\it's\f1.png")),
            r"C:/tmp/it'\''s/f1.png"
        );
    }

    #[test]
    fn empty_frames_error() {
        let path = std::env::temp_dir().join("hookecho_gif_empty.gif");
        assert!(encode_gif(&[], 100, &path).is_err());
        assert!(encode_mp4(&[], 5, &path).is_err());
        assert!(encode_gif_timed(std::iter::empty(), &[], &path).is_err());
    }

    #[test]
    fn encodes_mp4_when_ffmpeg_present() {
        // Skip cleanly if ffmpeg isn't installed (CI without it still passes).
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let mut a = RgbaImage::new(16, 16);
            for p in a.pixels_mut() {
                *p = image::Rgba([200, 40, 40, 255]);
            }
            let path = std::env::temp_dir().join("hookecho_mp4_test.mp4");
            encode_mp4(&[a.clone(), a], 5, &path).expect("mp4 encode");
            let bytes = std::fs::read(&path).expect("mp4 written");
            // MP4 files carry an 'ftyp' box near the start.
            assert!(
                bytes.windows(4).take(64).any(|w| w == b"ftyp"),
                "looks like an MP4"
            );
        }
    }
}
