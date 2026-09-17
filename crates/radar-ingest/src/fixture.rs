//! ROADMAP_NEW B6.10: load a JSON fixture of [`RawProduct`]s for the `radar-ingest` binary's
//! replay mode. This exists because there is no live LDM adapter yet (see [`crate::ldm`]'s own
//! doc comment) — a fixture file is the only way to run this binary against *something* today,
//! whether that's a captured feed for a demo deployment or a hand-written product for testing a
//! client end to end against a real, running relay instead of only an in-process test server.
//!
//! The file is a plain JSON array of [`RawProduct`], newest last (replayed in file order):
//!
//! ```json
//! [
//!   {"site": "KTLX", "bytes": [8, 5, 0, 1, ...], "received_at": "2026-01-01T00:00:00Z"}
//! ]
//! ```

use crate::input::RawProduct;
use std::path::Path;

/// Read and parse a fixture file. Errors are wrapped with the path so a misconfigured
/// `RADAR_INGEST_REPLAY_FILE` fails with something actionable rather than a bare serde message.
pub fn load(path: &Path) -> anyhow::Result<Vec<RawProduct>> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading replay fixture {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("parsing replay fixture {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A unique scratch path per test, in the OS temp dir — avoids pulling in a `tempfile`
    /// dependency just for two tests that only need "a file path nothing else is using."
    fn scratch_path(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("radar-ingest-fixture-test-{name}-{n}.json"))
    }

    #[test]
    fn a_well_formed_fixture_loads_in_file_order() {
        let path = scratch_path("wellformed");
        std::fs::write(
            &path,
            r#"[
                {"site": "KTLX", "bytes": [1, 2, 3], "received_at": "2026-01-01T00:00:00Z"},
                {"site": "KOHX", "bytes": [4, 5], "received_at": "2026-01-01T00:00:01Z"}
            ]"#,
        )
        .unwrap();
        let products = load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(products.len(), 2);
        assert_eq!(products[0].site, "KTLX");
        assert_eq!(products[0].bytes, vec![1, 2, 3]);
        assert_eq!(products[1].site, "KOHX");
    }

    #[test]
    fn a_missing_file_names_the_path_in_the_error() {
        let path = scratch_path("missing");
        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn malformed_json_names_the_path_in_the_error() {
        let path = scratch_path("malformed");
        std::fs::write(&path, "not json").unwrap();
        let err = load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.to_string().contains("parsing replay fixture"));
    }
}
