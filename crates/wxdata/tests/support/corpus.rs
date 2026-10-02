//! Shared, test-only input contract. No archive listing or replacement golden is permitted.
use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub baseline_commit: String,
    pub fixtures: Vec<Fixture>,
    pub truth_snapshots: Vec<TruthSnapshot>,
    pub track_snapshots: Vec<TrackSnapshot>,
}

/// Original NWS damage-analysis files. Metadata is checked against the file itself;
/// these paths are neither point reports nor time-interpolated tornado positions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSnapshot {
    pub id: String,
    pub path: String,
    pub format: String,
    pub bytes: usize,
    pub sha256: String,
    pub source: TruthSource,
    pub event_name: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub evidence: String,
    pub expected_vertices: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TruthSnapshot {
    pub id: String,
    pub path: String,
    pub format: String,
    pub bytes: usize,
    pub sha256: String,
    pub request: TruthRequest,
    pub source: TruthSource,
    pub expected_features: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TruthSource {
    pub url: String,
    pub captured_at: DateTime<Utc>,
    pub attribution: String,
    pub license_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TruthRequest {
    WarningAt {
        at: DateTime<Utc>,
    },
    Reports {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
}

pub fn truth_url(request: &TruthRequest) -> String {
    let stamp = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H%%3A%M%%3A%SZ").to_string();
    match *request {
        TruthRequest::WarningAt { at } => format!(
            "https://mesonet.agron.iastate.edu/geojson/sbw.py?ts={}",
            stamp(at)
        ),
        TruthRequest::Reports { start, end } => format!(
            "https://mesonet.agron.iastate.edu/geojson/lsr.geojson?sts={}&ets={}",
            stamp(start),
            stamp(end)
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub id: String,
    pub tier: String,
    pub path: String,
    pub format: String,
    pub site: String,
    pub case_time: DateTime<Utc>,
    pub sha256: String,
    pub bytes: usize,
    pub source: Source,
    pub transform: Transform,
    pub tags: Vec<String>,
    pub checks: Vec<String>,
    pub expected: Option<Expected>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub object_key: String,
    pub sha256: String,
    pub bytes: usize,
    pub acquisition_time: DateTime<Utc>,
    pub captured_at: DateTime<Utc>,
    pub attribution: String,
    pub license_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub kind: String,
    pub records: Vec<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub elevations: Vec<f32>,
    pub az_bins: usize,
    pub gate_count: usize,
    pub reflectivity_sha256: String,
    pub populated_bins: usize,
    pub max_dbz: f32,
}

pub fn manifest() -> Manifest {
    let m: Manifest = serde_json::from_str(include_str!("../data/corpus/manifest.json"))
        .expect("pinned corpus manifest must parse");
    validate(&m).expect("pinned corpus manifest must be valid");
    m
}

pub fn validate(m: &Manifest) -> Result<()> {
    ensure!(m.schema_version == 3, "unsupported corpus schema");
    ensure!(!m.baseline_commit.is_empty(), "missing algorithm baseline");
    ensure!(!m.fixtures.is_empty(), "empty corpus");
    let mut ids = HashSet::new();
    let mut contexts = HashSet::new();
    let hash = |h: &str| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    };
    for f in &m.fixtures {
        ensure!(ids.insert(&f.id), "duplicate fixture {}", f.id);
        ensure!(
            !f.id.is_empty()
                && f.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "invalid fixture ID"
        );
        let parts: Vec<_> = Path::new(&f.path).components().collect();
        ensure!(
            parts.len() == 1
                && matches!(parts[0], Component::Normal(_))
                && !f.path.contains(['/', '\\', ':']),
            "unsafe fixture path {}",
            f.path
        );
        ensure!(f.format == "nexrad-archive-ii", "unsupported format");
        ensure!(
            matches!(f.tier.as_str(), "offline" | "cached"),
            "unknown fixture tier"
        );
        ensure!(
            hash(&f.sha256) && hash(&f.source.sha256),
            "invalid checksum for {}",
            f.id
        );
        ensure!(
            f.bytes > 24
                && f.bytes <= 200_000_000
                && f.source.bytes >= f.bytes
                && f.source.bytes <= 200_000_000,
            "invalid size"
        );
        ensure!(
            !f.tags.is_empty() && !f.checks.is_empty(),
            "missing scientific checks"
        );
        ensure!(
            !f.source.attribution.is_empty() && f.source.license_url.starts_with("https://"),
            "missing attribution/license"
        );
        ensure!(
            f.source.captured_at >= f.source.acquisition_time
                && f.case_time >= f.source.acquisition_time,
            "invalid source clock"
        );
        let name = f
            .source
            .object_key
            .rsplit('/')
            .next()
            .context("object name")?;
        let id = nexrad_data::aws::archive::Identifier::new(name.into());
        ensure!(
            id.date_time() == Some(f.source.acquisition_time) && id.site() == Some(f.site.as_str()),
            "object clock/site mismatch"
        );
        let key = format!(
            "{}/{}/{}",
            f.source.acquisition_time.format("%Y/%m/%d"),
            f.site,
            name
        );
        ensure!(key == f.source.object_key, "object prefix mismatch");
        ensure!(
            f.source.url == format!("https://unidata-nexrad-level2.s3.amazonaws.com/{key}"),
            "source URL mismatch"
        );
        if f.tier == "cached" {
            ensure!(
                contexts.insert((&f.site, f.case_time)),
                "ambiguous historic case context"
            );
            ensure!(
                f.transform.kind == "identity" && f.transform.records.is_empty(),
                "cached input must be unaltered"
            );
            ensure!(
                f.path == name && f.bytes == f.source.bytes && f.sha256 == f.source.sha256,
                "cached input differs from source"
            );
        } else {
            ensure!(
                f.transform.kind == "ldm-record-subset" && !f.transform.records.is_empty(),
                "offline transform missing"
            );
            ensure!(
                f.transform.records.windows(2).all(|w| w[0] < w[1]),
                "records must be unique and ascending"
            );
            let e = f
                .expected
                .as_ref()
                .context("required offline expectation missing")?;
            ensure!(
                hash(&e.reflectivity_sha256)
                    && e.max_dbz.is_finite()
                    && !e.elevations.is_empty()
                    && e.elevations.iter().all(|v| v.is_finite()),
                "invalid expected science"
            );
            ensure!(
                e.az_bins > e.populated_bins && e.populated_bins > 0 && e.gate_count > 0,
                "expected partial coverage"
            );
        }
    }
    let mut requests = HashSet::new();
    for f in &m.truth_snapshots {
        ensure!(ids.insert(&f.id), "duplicate truth identity");
        let parts: Vec<_> = Path::new(&f.path).components().collect();
        ensure!(
            parts.len() == 1
                && matches!(parts[0], Component::Normal(_))
                && !f.path.contains(['/', '\\', ':']),
            "unsafe truth path"
        );
        ensure!(
            f.format == "geojson-feature-collection"
                && hash(&f.sha256)
                && f.bytes > 0
                && f.bytes <= 2_000_000,
            "invalid truth snapshot"
        );
        let url = truth_url(&f.request);
        ensure!(
            requests.insert(url.clone()) && url == f.source.url,
            "ambiguous or incorrect truth request"
        );
        let time = match f.request {
            TruthRequest::WarningAt { at } => at,
            TruthRequest::Reports { start, end } => {
                ensure!(start < end, "invalid report window");
                end
            }
        };
        ensure!(
            f.source.captured_at >= time
                && !f.source.attribution.is_empty()
                && f.source.license_url == "https://mesonet.agron.iastate.edu/disclaimer.php",
            "truth attribution/time missing"
        );
    }
    ensure!(
        !m.truth_snapshots.is_empty(),
        "required truth snapshots missing"
    );
    ensure!(
        !m.track_snapshots.is_empty(),
        "required damage tracks missing"
    );
    for f in &m.track_snapshots {
        ensure!(ids.insert(&f.id), "duplicate track identity");
        let parts: Vec<_> = Path::new(&f.path).components().collect();
        ensure!(
            parts.len() == 1
                && matches!(parts[0], Component::Normal(_))
                && !f.path.contains(['/', '\\', ':']),
            "unsafe track path"
        );
        ensure!(
            f.format == "nws-damage-track-kmz"
                && hash(&f.sha256)
                && f.bytes > 0
                && f.bytes <= 2_000_000
                && f.expected_vertices >= 2,
            "invalid track integrity contract"
        );
        ensure!(
            f.source.url
                == format!(
                    "https://www.weather.gov/source/dmx/IowaTors/2021/{}",
                    f.path
                )
                && f.path.ends_with(".kmz")
                && requests.insert(f.source.url.clone()),
            "incorrect or duplicate damage-track source"
        );
        ensure!(
            f.start < f.end
                && f.source.captured_at >= f.end
                && !f.event_name.is_empty()
                && !f.evidence.is_empty()
                && !f.source.attribution.is_empty()
                && f.source.license_url == "https://www.weather.gov/disclaimer",
            "damage-track time/attribution missing"
        );
    }
    Ok(())
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("HOOKECHO_CORPUS_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/scientific-corpus")
        })
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn verify(f: &Fixture, bytes: &[u8]) -> Result<()> {
    verify_bytes(&f.id, f.bytes, &f.sha256, bytes)
}

pub fn verify_bytes(id: &str, size: usize, sha256: &str, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() == size,
        "{}: size mismatch, got {} expected {}",
        id,
        bytes.len(),
        size
    );
    ensure!(
        digest(bytes) == sha256,
        "{}: SHA-256 mismatch; refusing a replacement golden",
        id
    );
    Ok(())
}

pub fn read_truth(f: &TruthSnapshot) -> Result<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/corpus")
        .join(&f.path);
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{}: required truth snapshot missing at {}",
            f.id,
            path.display()
        )
    })?;
    verify_bytes(&f.id, f.bytes, &f.sha256, &bytes)?;
    let json = String::from_utf8(bytes)?;
    let collection: serde_json::Value = serde_json::from_str(&json)?;
    ensure!(
        collection["type"] == "FeatureCollection"
            && collection["features"]
                .as_array()
                .is_some_and(|v| v.len() == f.expected_features),
        "{}: pinned truth collection shape/count changed",
        f.id
    );
    Ok(json)
}

pub fn track_properties(description: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut fields = std::collections::HashMap::new();
    // This pinned NWS export encodes attributes in a two-column HTML table, not
    // ExtendedData. Fail if the shape changes rather than guessing absent times.
    for row in description.split("<tr>").skip(1) {
        let row = row.split_once("</tr>").context("unterminated track row")?.0;
        let cells: Vec<_> = row
            .split("<td>")
            .skip(1)
            .map(|s| s.split_once("</td>").map(|(value, _)| value))
            .collect::<Option<Vec<_>>>()
            .context("unterminated track cell")?;
        ensure!(cells.len() == 2, "unexpected track table row");
        let key = cells[0]
            .strip_prefix("<b>")
            .and_then(|s| s.strip_suffix("</b>"))
            .context("unexpected track field")?;
        ensure!(
            fields.insert(key.to_owned(), cells[1].to_owned()).is_none(),
            "duplicate track field"
        );
    }
    ensure!(!fields.is_empty(), "damage track has no source attributes");
    Ok(fields)
}

pub fn read_track(f: &TrackSnapshot) -> Result<Vec<[f64; 2]>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/corpus")
        .join(&f.path);
    let bytes =
        std::fs::read(&path).with_context(|| format!("{}: required damage track missing", f.id))?;
    verify_bytes(&f.id, f.bytes, &f.sha256, &bytes)?;
    let mut features = wxdata::kml::parse_kmz(&bytes)?;
    ensure!(features.len() == 1, "{}: expected one track", f.id);
    let feature = features.pop().unwrap();
    let fields = track_properties(
        feature
            .properties
            .get("description")
            .and_then(|value| value.as_str())
            .context("track description missing")?,
    )?;
    for (key, expected) in [
        ("event_id", f.event_name.clone()),
        ("starttime", f.start.format("%Y-%m-%d %H:%M:%S").to_string()),
        ("endtime", f.end.format("%Y-%m-%d %H:%M:%S").to_string()),
        ("comments", f.evidence.clone()),
    ] {
        ensure!(
            fields.get(key) == Some(&expected),
            "{}: source {key} differs from manifest",
            f.id
        );
    }
    let wxdata::gis::Geometry::LineString(line) = feature.geometry else {
        bail!("{}: damage track is not a line", f.id);
    };
    ensure!(
        line.len() == f.expected_vertices
            && line.iter().all(|p| p[0].is_finite()
                && p[1].is_finite()
                && (-180.0..=180.0).contains(&p[0])
                && (-90.0..=90.0).contains(&p[1])),
        "{}: damage track vertices changed",
        f.id
    );
    Ok(line)
}

pub fn warnings_at(at: DateTime<Utc>) -> Vec<wxdata::overlay::GeoFeature> {
    let m = manifest();
    let f = m
        .truth_snapshots
        .iter()
        .find(|f| matches!(f.request, TruthRequest::WarningAt { at: t } if t == at))
        .expect("warning instant must name a pinned snapshot");
    wxdata::archive_warnings::parse(&read_truth(f).expect("verified warning snapshot"))
        .expect("warning snapshot must decode")
}

pub fn reports_between(start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<wxdata::spc::StormReport> {
    let m = manifest();
    let f = m.truth_snapshots.iter().find(|f| matches!(f.request, TruthRequest::Reports { start: s, end: e } if s == start && e == end)).expect("report window must name a pinned snapshot");
    wxdata::lsr::parse(&read_truth(f).expect("verified report snapshot"))
}

pub fn read(f: &Fixture, cache: &Path) -> Result<Vec<u8>> {
    let root = if f.tier == "offline" {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/corpus")
    } else {
        cache.to_path_buf()
    };
    let path = root.join(&f.path);
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{}: required fixture missing/unreadable at {}; run python scripts/corpus/provision.py",
            f.id,
            path.display()
        )
    })?;
    verify(f, &bytes)?;
    Ok(bytes)
}

/// Existing network tests may fetch exact pinned objects. A configured cache is mandatory:
/// missing or damaged cached inputs fail instead of quietly falling back to the network.
pub async fn historic_volume(site: &str, when: DateTime<Utc>) -> wxdata::level2::Scan {
    let m = manifest();
    let f = m
        .fixtures
        .iter()
        .find(|f| f.tier == "cached" && f.site == site && f.case_time == when)
        .expect("historic case must name a pinned manifest input");
    let bytes = if std::env::var_os("HOOKECHO_CORPUS_CACHE").is_some() {
        read(f, &cache_dir()).expect("verified cached input")
    } else {
        fetch(f).await.expect("verified pinned archive input")
    };
    eprintln!("{}: {} SHA-256 {}", f.id, f.source.object_key, f.sha256);
    wxdata::task::blocking(move || wxdata::level2::decode_volume(bytes))
        .await
        .expect("decode task")
        .expect("pinned volume must decode")
}

async fn fetch(f: &Fixture) -> Result<Vec<u8>> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let mut last = None;
    for _ in 0..3 {
        let result = async {
            let bytes = http
                .get(&f.source.url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            Ok::<_, anyhow::Error>(bytes.to_vec())
        }
        .await;
        match result {
            Ok(bytes) => {
                // Integrity failure is permanent; retries must not disguise changed inputs.
                verify(f, &bytes)?;
                return Ok(bytes);
            }
            Err(e) => last = Some(e),
        }
    }
    bail!(
        "{}: archive request failed after three attempts: {:?}",
        f.id,
        last
    )
}
