//! Configuration for a live LDM/IDD upstream peer (ROADMAP_NEW B6.11 step 5).
//!
//! **Status: configuration only, no live protocol client.** This module defines the external,
//! deployment-provided configuration shape a live LDM adapter needs — host/port, the feed
//! pattern, and an optional site allowlist — satisfying B6.2's "treat LDM access as deployment
//! configuration, not an entitlement" and "make LDM upstream configuration external" requirements.
//!
//! It does **not** implement the LDM6/7 wire protocol itself. That protocol (`ldmd`'s peer
//! protocol, historically ONC RPC/XDR-based, later LDM7's VCMTP-based product delivery) is
//! stateful, undocumented outside Unidata's own C reference implementation, and — critically —
//! cannot be implemented blind: there is no way to validate a from-scratch client's framing,
//! handshake, or `HEREIS`/`COMINGSOON` product-delivery semantics without a real upstream peer to
//! connect to and observe. No such peer or credentials are available in this environment. This is
//! the "unavailable credentials/upstream LDM access" external blocker anticipated by ROADMAP_NEW's
//! own B6 planning, not an oversight.
//!
//! What this means concretely for B6:
//!
//! - The adapter *boundary* this would plug into already exists and is exercised by real tests —
//!   [`crate::input::InputAdapter`], currently implemented by
//!   [`crate::input::ReplayInputAdapter`]. A live LDM client, once built against a real peer,
//!   implements the same trait and requires no changes to [`crate::pipeline::Pipeline`],
//!   [`crate::rechunk::Rechunker`], or [`crate::server`] — all of B6.11 steps 1-4 downstream of
//!   ingestion are already provider-agnostic.
//! - The remaining B6.11 steps (6: client `HookEchoRelayLevel2Provider`, 7: dual-feed operation,
//!   8: failover arbiter, 9: mid-volume continuation, 10: TGFTP degraded fallback, 11:
//!   diagnostics, 12: chaos/replay tests) do not require a *live* LDM connection to build or test
//!   — they only require *some* [`crate::input::InputAdapter`] feeding the pipeline, which
//!   [`crate::input::ReplayInputAdapter`] already provides. Work continues there rather than
//!   blocking on LDM access.
//! - When a real LDM peer becomes available (e.g. a Unidata IDD relay agreement, or a third-party
//!   LDM7 relay), implementing [`crate::input::InputAdapter`] against it — likely via FFI to the
//!   reference `ldm` C toolkit rather than a from-scratch Rust reimplementation of the wire
//!   protocol, given the validation problem above — is the only remaining piece; everything it
//!   feeds into is already built and tested.

use std::env;

/// External configuration for a live LDM/IDD upstream peer. Constructed from environment
/// variables so a deployment supplies its own permitted peer rather than this crate assuming one
/// is available (ROADMAP_NEW B6.2/B6.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdmSourceConfig {
    /// Upstream LDM host, e.g. an IDD relay a deployment has an explicit feed agreement with.
    pub host: String,
    /// LDM's conventional port (388), overridable per deployment.
    pub port: u16,
    /// The LDM feed type/pattern to request — `NEXRAD2` for Level II (historically also
    /// identified as FT28/CRAFT/NEXRD2), per ROADMAP_NEW B6.2.
    pub feed_pattern: String,
    /// Restrict ingestion to these sites; `None` accepts every site the feed offers. Distinct
    /// from [`crate::store::IngestStore`]'s own allowlist (this one shapes what is *requested*
    /// from the upstream peer; the store's shapes what is *admitted* after arrival — a deployment
    /// may reasonably want both, or only one).
    pub site_allowlist: Option<Vec<String>>,
}

/// Error reading [`LdmSourceConfig`] from the environment.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LdmConfigError {
    #[error("{0} must be set (no permitted LDM upstream peer is assumed by default)")]
    MissingHost(&'static str),
    #[error("{0} is not a valid port number: {1}")]
    InvalidPort(&'static str, String),
}

const HOST_VAR: &str = "RADAR_INGEST_LDM_HOST";
const PORT_VAR: &str = "RADAR_INGEST_LDM_PORT";
const FEED_VAR: &str = "RADAR_INGEST_LDM_FEED";
const SITES_VAR: &str = "RADAR_INGEST_LDM_SITES";

impl LdmSourceConfig {
    /// Reads [`HOST_VAR`]/[`PORT_VAR`]/[`FEED_VAR`]/[`SITES_VAR`] from the process environment.
    /// Returns an error rather than a default host — silently assuming an upstream peer is
    /// available is exactly what ROADMAP_NEW B6.2 says not to do.
    pub fn from_env() -> Result<Self, LdmConfigError> {
        let host = env::var(HOST_VAR).map_err(|_| LdmConfigError::MissingHost(HOST_VAR))?;
        let port = match env::var(PORT_VAR) {
            Ok(raw) => raw
                .parse()
                .map_err(|_| LdmConfigError::InvalidPort(PORT_VAR, raw))?,
            Err(_) => 388, // LDM's conventional port
        };
        let feed_pattern = env::var(FEED_VAR).unwrap_or_else(|_| "NEXRAD2".to_string());
        let site_allowlist = env::var(SITES_VAR).ok().map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| !s.is_empty())
                .collect()
        });
        Ok(Self {
            host,
            port,
            feed_pattern,
            site_allowlist,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Environment variables are process-global, so these tests serialize on a mutex rather than
    // risking cross-test interference under `cargo test`'s default parallel execution.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_env() {
        for var in [HOST_VAR, PORT_VAR, FEED_VAR, SITES_VAR] {
            env::remove_var(var);
        }
    }

    #[test]
    fn missing_host_is_an_error_not_an_assumed_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        assert_eq!(
            LdmSourceConfig::from_env(),
            Err(LdmConfigError::MissingHost(HOST_VAR))
        );
    }

    #[test]
    fn a_configured_host_gets_ldm_conventional_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var(HOST_VAR, "ldm.example.edu");
        let config = LdmSourceConfig::from_env().unwrap();
        assert_eq!(config.host, "ldm.example.edu");
        assert_eq!(config.port, 388);
        assert_eq!(config.feed_pattern, "NEXRAD2");
        assert_eq!(config.site_allowlist, None);
        clear_env();
    }

    #[test]
    fn explicit_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var(HOST_VAR, "ldm.example.edu");
        env::set_var(PORT_VAR, "1388");
        env::set_var(FEED_VAR, "NEXRAD2|NOTHER");
        env::set_var(SITES_VAR, "ktlx, kohx ,");
        let config = LdmSourceConfig::from_env().unwrap();
        assert_eq!(config.port, 1388);
        assert_eq!(config.feed_pattern, "NEXRAD2|NOTHER");
        assert_eq!(
            config.site_allowlist,
            Some(vec!["KTLX".to_string(), "KOHX".to_string()])
        );
        clear_env();
    }

    #[test]
    fn an_unparseable_port_is_a_named_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var(HOST_VAR, "ldm.example.edu");
        env::set_var(PORT_VAR, "not-a-port");
        assert_eq!(
            LdmSourceConfig::from_env(),
            Err(LdmConfigError::InvalidPort(
                PORT_VAR,
                "not-a-port".to_string()
            ))
        );
        clear_env();
    }
}
