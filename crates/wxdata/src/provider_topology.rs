//! Declared acquisition dependencies, separate from delivery labels and radial identities.
//! Different declarations never establish independent redundancy: they are configuration
//! evidence, not an audit of shared power, networks, radar origin or upstream infrastructure.

use serde::{Deserialize, Serialize};

pub const UNIDATA_AWS_DOMAIN: &str = "unidata-level2-aws";
pub const NOAA_TGFTP_DOMAIN: &str = "noaa-level2-tgftp";
pub const MAX_DOMAINS: usize = 8;
pub const MAX_DOMAIN_BYTES: usize = 96;
pub const MAX_DECLARATION_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    Live,
    Replay,
    Idle,
    #[default]
    Unknown,
}

impl InputMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "live input",
            Self::Replay => "replay input",
            Self::Idle => "no input",
            Self::Unknown => "input mode unknown",
        }
    }
}

/// Bounded identifiers that operators must keep non-secret. URL, path and control syntax is
/// rejected; arbitrary identifier text cannot be checked for secrets. Equality is exact after
/// sorted duplicate removal. Deployment display labels are not inferred as dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DeclarationDto", into = "DeclarationDto")]
pub struct UpstreamDeclaration {
    input_mode: InputMode,
    failure_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeclarationDto {
    schema_version: u16,
    #[serde(default)]
    input_mode: InputMode,
    #[serde(default)]
    failure_domains: Vec<String>,
}

impl UpstreamDeclaration {
    pub fn new(
        input_mode: InputMode,
        mut failure_domains: Vec<String>,
    ) -> Result<Self, &'static str> {
        if failure_domains.len() > MAX_DOMAINS {
            return Err("too many failure domains");
        }
        if failure_domains.iter().any(|id| {
            id.is_empty()
                || id.len() > MAX_DOMAIN_BYTES
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
        }) {
            return Err("failure domains must be bounded, non-secret lowercase identifiers");
        }
        failure_domains.sort();
        failure_domains.dedup();
        Ok(Self {
            input_mode,
            failure_domains,
        })
    }

    pub fn unknown() -> Self {
        Self {
            input_mode: InputMode::Unknown,
            failure_domains: Vec::new(),
        }
    }
    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }
    pub fn failure_domains(&self) -> &[String] {
        &self.failure_domains
    }
}

impl TryFrom<DeclarationDto> for UpstreamDeclaration {
    type Error = &'static str;
    fn try_from(dto: DeclarationDto) -> Result<Self, Self::Error> {
        if dto.schema_version != 1 {
            return Err("unsupported upstream declaration version");
        }
        Self::new(dto.input_mode, dto.failure_domains)
    }
}

impl From<UpstreamDeclaration> for DeclarationDto {
    fn from(value: UpstreamDeclaration) -> Self {
        Self {
            schema_version: 1,
            input_mode: value.input_mode,
            failure_domains: value.failure_domains,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationEvidence {
    Adapter,
    RelayHttp,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationUnavailable {
    NotDeclared,
    Pending,
    HttpFailure,
    Timeout,
    TooLarge,
    Invalid,
}

impl DeclarationUnavailable {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotDeclared => "not declared",
            Self::Pending => "metadata request pending",
            Self::HttpFailure => "metadata endpoint unavailable",
            Self::Timeout => "metadata request timed out",
            Self::TooLarge => "metadata exceeded size limit",
            Self::Invalid => "metadata invalid or unsupported",
        }
    }
}

/// Metadata evidence only. Does not contain the relay endpoint, source_id, user paths or raw
/// transport errors, and does not grant capabilities or change scientific receipt identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderTopology {
    pub declaration: Option<UpstreamDeclaration>,
    pub evidence: DeclarationEvidence,
    pub unavailable: Option<DeclarationUnavailable>,
}

impl Default for ProviderTopology {
    fn default() -> Self {
        Self::unknown(DeclarationUnavailable::NotDeclared)
    }
}

impl ProviderTopology {
    pub fn unknown(reason: DeclarationUnavailable) -> Self {
        Self {
            declaration: None,
            evidence: DeclarationEvidence::Unavailable,
            unavailable: Some(reason),
        }
    }
    pub fn adapter(domain: &str) -> Self {
        Self {
            declaration: Some(
                UpstreamDeclaration::new(InputMode::Live, vec![domain.into()])
                    .expect("adapter domain constant"),
            ),
            evidence: DeclarationEvidence::Adapter,
            unavailable: None,
        }
    }
    pub fn relay(declaration: UpstreamDeclaration) -> Self {
        Self {
            declaration: Some(declaration),
            evidence: DeclarationEvidence::RelayHttp,
            unavailable: None,
        }
    }
    pub fn detail(&self) -> String {
        match &self.declaration {
            None => format!(
                "unknown ({})",
                self.unavailable
                    .unwrap_or(DeclarationUnavailable::NotDeclared)
                    .label()
            ),
            Some(declaration) => format!(
                "{}; domains: {}; {}",
                declaration.input_mode.label(),
                if declaration.failure_domains.is_empty() {
                    "unknown".to_string()
                } else {
                    declaration.failure_domains.join(", ")
                },
                match self.evidence {
                    DeclarationEvidence::Adapter => "adapter declaration",
                    DeclarationEvidence::RelayHttp => "relay HTTP declaration",
                    DeclarationEvidence::Unavailable => "evidence unavailable",
                }
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "relationship", rename_all = "snake_case")]
pub enum UpstreamRelationship {
    Unknown,
    SharedDeclaredDomains { domains: Vec<String> },
    DistinctDeclaredDomains,
}

impl UpstreamRelationship {
    pub fn compare(primary: &ProviderTopology, backup: &ProviderTopology) -> Self {
        let (Some(a), Some(b)) = (&primary.declaration, &backup.declaration) else {
            return Self::Unknown;
        };
        if a.failure_domains.is_empty() || b.failure_domains.is_empty() {
            return Self::Unknown;
        }
        let shared: Vec<_> = a
            .failure_domains
            .iter()
            .filter(|id| b.failure_domains.contains(id))
            .cloned()
            .collect();
        if shared.is_empty() {
            Self::DistinctDeclaredDomains
        } else {
            Self::SharedDeclaredDomains { domains: shared }
        }
    }
    pub fn detail(&self) -> String {
        match self {
            Self::Unknown => "unknown; independent redundancy not established".into(),
            Self::SharedDeclaredDomains { domains } => format!(
                "shared declared upstream: {}; upstream outage may affect both paths",
                domains.join(", ")
            ),
            Self::DistinctDeclaredDomains => {
                "declared domains differ; independent redundancy not established".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_topology_declarations_are_bounded_and_reject_urls_and_future_versions() {
        for bad in [
            "",
            "https://user:password@relay.example",
            "upstream\n",
            "DOMAIN",
            "a?token=secret",
            "../secret/path",
        ] {
            assert!(UpstreamDeclaration::new(InputMode::Live, vec![bad.into()]).is_err());
        }
        assert!(UpstreamDeclaration::new(InputMode::Live, vec!["a".repeat(97)]).is_err());
        assert!(UpstreamDeclaration::new(InputMode::Live, vec!["a".into(); 9]).is_err());
        assert!(serde_json::from_str::<UpstreamDeclaration>(
            r#"{"schema_version":2,"input_mode":"live","failure_domains":[]}"#
        )
        .is_err());
        let declaration =
            UpstreamDeclaration::new(InputMode::Replay, vec!["b".into(), "a".into(), "a".into()])
                .unwrap();
        assert_eq!(declaration.failure_domains(), &["a", "b"]);
        let roundtrip: UpstreamDeclaration =
            serde_json::from_str(&serde_json::to_string(&declaration).unwrap()).unwrap();
        assert_eq!(roundtrip, declaration);
        let omitted: UpstreamDeclaration = serde_json::from_str(r#"{"schema_version":1}"#).unwrap();
        assert_eq!(omitted, UpstreamDeclaration::unknown());
    }

    #[test]
    fn provider_topology_relations_compare_dependencies_without_certifying_independence() {
        let primary = ProviderTopology::adapter(UNIDATA_AWS_DOMAIN);
        let shared = ProviderTopology::relay(
            UpstreamDeclaration::new(
                InputMode::Live,
                vec![UNIDATA_AWS_DOMAIN.into(), "deployment-network".into()],
            )
            .unwrap(),
        );
        assert_eq!(
            UpstreamRelationship::compare(&primary, &shared),
            UpstreamRelationship::SharedDeclaredDomains {
                domains: vec![UNIDATA_AWS_DOMAIN.into()]
            }
        );
        let distinct = ProviderTopology::relay(
            UpstreamDeclaration::new(InputMode::Live, vec!["idd-peer-a".into()]).unwrap(),
        );
        assert_eq!(
            UpstreamRelationship::compare(&primary, &distinct),
            UpstreamRelationship::DistinctDeclaredDomains
        );
        assert!(UpstreamRelationship::compare(&primary, &distinct)
            .detail()
            .contains("not established"));
        for unknown in [
            ProviderTopology::default(),
            ProviderTopology::relay(UpstreamDeclaration::unknown()),
        ] {
            assert_eq!(
                UpstreamRelationship::compare(&primary, &unknown),
                UpstreamRelationship::Unknown
            );
        }
    }
}
