//! Plain-language storm digest: turn the in-view alerts + storm reports into a short readable
//! briefing. Works fully offline (templated summary); if an Anthropic key is set, Claude rewrites
//! the same facts into friendlier prose.

/// One active alert to summarize.
pub struct AlertLine {
    pub event: String,
    pub area: String,
}

/// A deterministic, offline plain-language summary. Also serves as the exact fact list handed to
/// the LLM, so the two never disagree on substance.
pub fn templated(alerts: &[AlertLine], reports: [usize; 3]) -> String {
    let mut out = String::new();
    if alerts.is_empty() {
        out.push_str("No active warnings or watches in view.");
    } else {
        // Count by event type.
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for a in alerts {
            *counts.entry(a.event.as_str()).or_default() += 1;
        }
        out.push_str("Active in view: ");
        let parts: Vec<String> = counts.iter().map(|(e, n)| format!("{n} {e}")).collect();
        out.push_str(&parts.join(", "));
        out.push('.');
        // Name a few specific areas for the highest-priority events.
        let mut areas: Vec<&str> = alerts
            .iter()
            .filter(|a| a.event.contains("Tornado") || a.event.contains("Severe"))
            .map(|a| a.area.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        areas.dedup();
        if !areas.is_empty() {
            out.push_str(" Affected: ");
            out.push_str(&areas.into_iter().take(4).collect::<Vec<_>>().join("; "));
            out.push('.');
        }
    }
    let [tor, wind, hail] = reports;
    if tor + wind + hail > 0 {
        out.push_str(&format!(
            " Today's storm reports: {tor} tornado, {wind} wind, {hail} hail."
        ));
    }
    out
}

/// Which model rewrites the templated facts (Settings > General > AI).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Provider {
    /// Claude, through the Anthropic API.
    #[default]
    Anthropic,
    /// Gemini, through a Google AI Studio API key.
    Gemini,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::Gemini];

    /// The model's name, as the digest says who wrote it.
    pub fn model_name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Claude",
            Provider::Gemini => "Gemini",
        }
    }

    /// The provider's name in the settings picker.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Claude (Anthropic)",
            Provider::Gemini => "Gemini (Google AI Studio)",
        }
    }
}

/// The Gemini model the digest asks. Flash: fast and inexpensive, which a few sentences need.
pub const GEMINI_MODEL: &str = "gemini-3.8-flash";

/// The instruction both models get. `context` is the templated summary (the ground truth).
fn prompt(context: &str) -> String {
    format!(
        "You are a calm, plain-language weather briefer for the general public. In 2-4 short \
         sentences, explain what these active weather conditions mean and what people in the \
         area should do. Do not invent facts beyond what is given. Facts:\n\n{context}"
    )
}

/// Rewrite the templated facts with `provider`, using `key`. Returns the model's text, or an
/// error the caller can show.
pub async fn enhance(
    http: &reqwest::Client,
    provider: Provider,
    key: &str,
    context: &str,
) -> anyhow::Result<String> {
    match provider {
        Provider::Anthropic => claude(http, key, context).await,
        Provider::Gemini => gemini(http, key, context).await,
    }
}

/// Rewrite the templated facts into friendly prose with Claude.
pub async fn claude(http: &reqwest::Client, key: &str, context: &str) -> anyhow::Result<String> {
    let body = serde_json::json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 400,
        "messages": [{"role": "user", "content": prompt(context)}],
    });
    let resp = http
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic API error {status}{}", api_message(&body));
    }
    let text = resp.text().await?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let out = v["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("unexpected API response shape"))?;
    Ok(out.to_string())
}

/// The same rewrite with Gemini, using a Google AI Studio API key. The model's reasoning counts
/// against the output budget, so the budget leaves room for it well beyond the few sentences
/// of prose wanted; otherwise the answer could come back empty. No thinking setting is sent:
/// its form differs between Gemini generations, and the model's default suits this.
pub async fn gemini(http: &reqwest::Client, key: &str, context: &str) -> anyhow::Result<String> {
    let body = serde_json::json!({
        "contents": [{"role": "user", "parts": [{"text": prompt(context)}]}],
        "generationConfig": {
            "maxOutputTokens": 4096,
        },
    });
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent"
    );
    let resp = http
        .post(url)
        .header("x-goog-api-key", key)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API error {status}{}", api_message(&body));
    }
    let text = resp.text().await?;
    gemini_text(&text)
}

/// The reason an API gives with an error, as ": <message>", or nothing. Both providers answer
/// `{"error": {"message": ...}}`, and the message is what tells someone their key is wrong.
fn api_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .map(|m| format!(": {m}"))
        .unwrap_or_default()
}

/// The answer's text from a `generateContent` response: every text part of the first
/// candidate, joined. An answer stopped by a safety filter or with no text is an error.
fn gemini_text(json: &str) -> anyhow::Result<String> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let parts = v["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or_else(|| match v["candidates"][0]["finishReason"].as_str() {
            Some(reason) => anyhow::anyhow!("Gemini returned no text ({reason})"),
            None => anyhow::anyhow!("unexpected API response shape"),
        })?;
    let out: String = parts
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    if out.trim().is_empty() {
        anyhow::bail!("Gemini returned no text");
    }
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gemini_answer_is_read_from_its_parts() {
        let ok = r#"{"candidates":[{"content":{"role":"model","parts":[
            {"text":"Storms are moving east. "},{"text":"Stay weather-aware."}]},
            "finishReason":"STOP"}]}"#;
        assert_eq!(
            gemini_text(ok).unwrap(),
            "Storms are moving east. Stay weather-aware."
        );
        let blocked = r#"{"candidates":[{"finishReason":"SAFETY"}]}"#;
        assert!(gemini_text(blocked)
            .unwrap_err()
            .to_string()
            .contains("SAFETY"));
        let empty = r#"{"candidates":[{"content":{"parts":[{"text":"  "}]}}]}"#;
        assert!(gemini_text(empty).is_err());
        assert!(gemini_text("{}").is_err());
    }

    #[test]
    fn an_api_error_says_why() {
        let gemini = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(
            api_message(gemini),
            ": API key not valid. Please pass a valid API key."
        );
        let anthropic = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(api_message(anthropic), ": invalid x-api-key");
        assert_eq!(api_message("<html>"), "");
    }

    #[test]
    fn the_provider_defaults_to_claude_so_existing_keys_keep_working() {
        assert_eq!(Provider::default(), Provider::Anthropic);
        assert_eq!(Provider::Gemini.model_name(), "Gemini");
    }

    #[test]
    fn templated_summarizes_and_counts() {
        let alerts = vec![
            AlertLine {
                event: "Tornado Warning".into(),
                area: "Cleveland Co.".into(),
            },
            AlertLine {
                event: "Tornado Warning".into(),
                area: "McClain Co.".into(),
            },
            AlertLine {
                event: "Severe Thunderstorm Warning".into(),
                area: "Grady Co.".into(),
            },
        ];
        let s = templated(&alerts, [1, 3, 2]);
        assert!(s.contains("2 Tornado Warning"), "counts events: {s}");
        assert!(s.contains("Cleveland Co."), "names affected areas: {s}");
        assert!(
            s.contains("1 tornado, 3 wind, 2 hail"),
            "storm report tally: {s}"
        );
    }

    #[test]
    fn templated_handles_quiet() {
        assert!(templated(&[], [0, 0, 0]).starts_with("No active"));
    }
}
