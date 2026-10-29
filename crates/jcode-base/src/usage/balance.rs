//! Provider credit balances and plan quotas, shared by `/usage` and the live
//! status chip.
//!
//! `/usage` lists every connected provider and renders one row per balance or
//! quota window; the status chip only needs the numbers for the provider in
//! use. Both read the same response through the same fetch and parse helpers
//! here, so the chip can never disagree with `/usage`.

use anyhow::{Context, Result};
use jcode_usage_types::UsageLimit;
use std::time::Duration;

use crate::provider_catalog::{self, OpenAiCompatibleProfile};

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// One provider balance response in both forms its consumers need: the exact
/// rows `/usage` renders, and the numeric amount the status chip shows.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BalanceSnapshot {
    pub lines: Vec<(String, String)>,
    /// `None` when the response carries no single remaining-credit number.
    pub amount: Option<BalanceAmount>,
    /// Usage windows for providers that meter a plan instead of a balance.
    pub limits: Vec<UsageLimit>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BalanceAmount {
    pub currency: String,
    pub amount: f64,
}

/// Remaining provider credit, rendered as a single UI row.
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceChip {
    pub provider: String,
    pub currency: String,
    pub amount: f64,
}

impl BalanceChip {
    /// `DeepSeek $28.11` / `Moonshot AI ¥120.00`.
    pub fn label(&self) -> String {
        format!("{} {}", self.provider, self.format_amount())
    }

    fn format_amount(&self) -> String {
        format_amount(&self.currency, self.amount)
    }

    /// Balance low enough that the user should notice before it runs out.
    pub fn is_low(&self) -> bool {
        self.amount.is_finite() && self.amount < 5.0
    }

    pub fn is_exhausted(&self) -> bool {
        self.amount.is_finite() && self.amount <= 0.0
    }
}

/// Plan usage windows for one provider, rendered as a single status row.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitChip {
    pub provider: String,
    pub windows: Vec<UsageLimit>,
}

impl LimitChip {
    /// Usage at or above this percent earns the warning color.
    pub const LOW_PERCENT: f32 = 70.0;
    /// Usage at or above this percent means the window is exhausted.
    pub const EXHAUSTED_PERCENT: f32 = 100.0;

    fn max_percent(&self) -> f32 {
        self.windows
            .iter()
            .map(|window| window.usage_percent)
            .fold(0.0, f32::max)
    }

    pub fn is_low(&self) -> bool {
        self.windows
            .iter()
            .any(|window| window.usage_percent >= Self::LOW_PERCENT)
    }

    pub fn is_exhausted(&self) -> bool {
        self.windows
            .iter()
            .any(|window| window.usage_percent >= Self::EXHAUSTED_PERCENT)
    }

    /// Highest window percentage, for logging and tests.
    pub fn max_usage_percent(&self) -> f32 {
        self.max_percent()
    }
}

/// What the status chip can meter for the provider in use.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderChip {
    Balance(BalanceChip),
    Limits(LimitChip),
}

impl ProviderChip {
    pub fn is_low(&self) -> bool {
        match self {
            Self::Balance(balance) => balance.is_low(),
            Self::Limits(limits) => limits.is_low(),
        }
    }

    pub fn is_exhausted(&self) -> bool {
        match self {
            Self::Balance(balance) => balance.is_exhausted(),
            Self::Limits(limits) => limits.is_exhausted(),
        }
    }
}

fn format_amount(currency: &str, amount: f64) -> String {
    let amount = if amount.is_finite() {
        amount.max(0.0)
    } else {
        0.0
    };
    let symbol = match currency.trim().to_ascii_uppercase().as_str() {
        "USD" => Some("$"),
        "CNY" | "RMB" => Some("¥"),
        _ => None,
    };
    match symbol {
        Some(symbol) => format!("{symbol}{amount:.2}"),
        None => format!("{amount:.2} {currency}"),
    }
}

/// Provider profile with a balance endpoint, addressed by profile id.
pub fn balance_profile(profile_id: &str) -> Option<&'static OpenAiCompatibleProfile> {
    let profile_id = profile_id.trim();
    provider_catalog::openai_compatible_profiles()
        .iter()
        .find(|profile| {
            profile.id.eq_ignore_ascii_case(profile_id) && has_balance_endpoint(profile.id)
        })
}

/// Profile whose display name (or id) matches a provider label from the wire.
pub fn balance_profile_for_label(label: &str) -> Option<&'static OpenAiCompatibleProfile> {
    let label = label.trim();
    provider_catalog::openai_compatible_profiles()
        .iter()
        .find(|profile| {
            has_balance_endpoint(profile.id)
                && (profile.display_name.eq_ignore_ascii_case(label)
                    || profile.id.eq_ignore_ascii_case(label))
        })
}

/// Extract a balance profile id from a session provider key such as
/// `openai-compatible:deepseek`, `deepseek`, or `deepseek@work`.
pub fn balance_profile_id_from_key(key: &str) -> Option<&'static str> {
    let key = key.trim();
    let key = key.split('@').next().unwrap_or(key);
    let key = key.rsplit(':').next().unwrap_or(key);
    balance_profile(key).map(|profile| profile.id)
}

fn has_balance_endpoint(profile_id: &str) -> bool {
    matches!(profile_id, "deepseek" | "moonshotai" | "opencode-go")
}

/// Fetch the live balance or plan quota for one profile. `Ok(None)` means the
/// provider has no metering API or no configured key, which is not an error.
pub async fn fetch_chip(profile_id: &str) -> Result<Option<ProviderChip>> {
    let Some(profile) = balance_profile(profile_id) else {
        return Ok(None);
    };
    let Some(api_key) = super::api_keys::configured_key(profile.api_key_env, profile.env_file)
    else {
        return Ok(None);
    };
    let snapshot = match profile.id {
        "deepseek" => deepseek_snapshot(&api_key).await?,
        "moonshotai" => {
            let resolved = provider_catalog::resolve_openai_compatible_profile(*profile);
            moonshot_snapshot(&api_key, &resolved.api_base).await?
        }
        "opencode-go" => opencode_go_snapshot(&api_key).await?,
        _ => return Ok(None),
    };
    if profile.id == "opencode-go" {
        if snapshot.limits.is_empty() {
            anyhow::bail!("no usage windows in response");
        }
        return Ok(Some(ProviderChip::Limits(LimitChip {
            provider: profile.display_name.to_string(),
            windows: snapshot.limits,
        })));
    }
    let Some(amount) = snapshot.amount else {
        anyhow::bail!("no numeric balance in response");
    };
    Ok(Some(ProviderChip::Balance(BalanceChip {
        provider: profile.display_name.to_string(),
        currency: amount.currency,
        amount: amount.amount,
    })))
}

async fn get_balance_json(url: &str, api_key: &str) -> Result<serde_json::Value> {
    let client = crate::provider::shared_http_client();
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Accept", "application/json")
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .context("balance request failed")?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }
    response.json().await.context("invalid balance response")
}

/// DeepSeek exposes a real balance endpoint for plain API keys.
pub(super) async fn deepseek_snapshot(api_key: &str) -> Result<BalanceSnapshot> {
    let json = get_balance_json("https://api.deepseek.com/user/balance", api_key).await?;
    deepseek_snapshot_from_json(&json)
}

fn deepseek_snapshot_from_json(json: &serde_json::Value) -> Result<BalanceSnapshot> {
    let mut lines = Vec::new();
    if let Some(available) = json.get("is_available").and_then(|v| v.as_bool()) {
        lines.push((
            "Key status".to_string(),
            if available {
                "valid (balance available)".to_string()
            } else {
                "balance exhausted".to_string()
            },
        ));
    }
    let mut amount = None;
    if let Some(infos) = json.get("balance_infos").and_then(|v| v.as_array()) {
        for info in infos {
            let currency = info.get("currency").and_then(|v| v.as_str()).unwrap_or("?");
            let total = info
                .get("total_balance")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let mut detail = format!("{} {}", total, currency);
            let topped_up = info.get("topped_up_balance").and_then(|v| v.as_str());
            let granted = info.get("granted_balance").and_then(|v| v.as_str());
            if let (Some(topped_up), Some(granted)) = (topped_up, granted) {
                detail.push_str(&format!(" ({} paid + {} granted)", topped_up, granted));
            }
            lines.push(("Balance".to_string(), detail));
            // The chip shows the first account's remaining credit; `/usage`
            // keeps listing every currency the endpoint reports.
            if amount.is_none()
                && let Some(value) = info.get("total_balance").and_then(number)
            {
                amount = Some(BalanceAmount {
                    currency: currency.to_string(),
                    amount: value,
                });
            }
        }
    }
    if lines.is_empty() {
        anyhow::bail!("no balance info in response");
    }
    Ok(BalanceSnapshot {
        lines,
        amount,
        limits: Vec::new(),
    })
}

/// OpenCode Go exposes plan usage as percentages for its rolling 5-hour,
/// weekly, and monthly windows. The endpoint is unofficial: there is no
/// documented public API, so a failure is surfaced as an error instead of
/// inventing numbers.
pub(super) async fn opencode_go_snapshot(api_key: &str) -> Result<BalanceSnapshot> {
    let json = get_balance_json("https://opencode.ai/zen/go/v1/usage", api_key).await?;
    opencode_go_snapshot_from_json(&json)
}

fn opencode_go_snapshot_from_json(json: &serde_json::Value) -> Result<BalanceSnapshot> {
    let usage = json
        .get("usage")
        .ok_or_else(|| anyhow::anyhow!("no usage in response"))?;
    let mut limits = Vec::new();
    for (key, name) in [
        ("rolling", "5-hour"),
        ("weekly", "Weekly"),
        ("monthly", "Monthly"),
    ] {
        let Some(window) = usage.get(key) else {
            continue;
        };
        let Some(percent) = window.get("percent").and_then(number) else {
            continue;
        };
        let resets_at = ["resetsAt", "resets_at"].iter().find_map(|field| {
            window
                .get(*field)
                .and_then(super::model::reset_timestamp_from_json_value)
        });
        limits.push(UsageLimit {
            name: name.to_string(),
            usage_percent: percent.clamp(0.0, 100.0) as f32,
            resets_at,
        });
    }
    if limits.is_empty() {
        anyhow::bail!("no usage windows in response");
    }
    Ok(BalanceSnapshot {
        lines: Vec::new(),
        amount: None,
        limits,
    })
}

/// Moonshot exposes `GET /v1/users/me/balance` for plain API keys.
pub(super) async fn moonshot_snapshot(api_key: &str, api_base: &str) -> Result<BalanceSnapshot> {
    let base = api_base.trim_end_matches('/');
    let json = get_balance_json(&format!("{base}/users/me/balance"), api_key).await?;
    moonshot_snapshot_from_json(&json, api_base)
}

fn moonshot_snapshot_from_json(
    json: &serde_json::Value,
    api_base: &str,
) -> Result<BalanceSnapshot> {
    let data = json
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("no balance data in response"))?;
    // Moonshot reports the currency only through the account's endpoint, so
    // derive it from the API host the profile actually points at.
    let currency = if api_base.contains("moonshot.cn") {
        "CNY"
    } else {
        "USD"
    };

    let mut lines = Vec::new();
    let mut amount = None;
    if let Some(available) = data.get("available_balance").and_then(|v| v.as_f64()) {
        lines.push((
            "Balance".to_string(),
            format!("{:.2} {} available", available, currency),
        ));
        lines.push((
            "Key status".to_string(),
            if available > 0.0 {
                "valid (balance available)".to_string()
            } else {
                "balance exhausted".to_string()
            },
        ));
        amount = Some(BalanceAmount {
            currency: currency.to_string(),
            amount: available,
        });
    }
    if let (Some(cash), Some(voucher)) = (
        data.get("cash_balance").and_then(|v| v.as_f64()),
        data.get("voucher_balance").and_then(|v| v.as_f64()),
    ) {
        lines.push((
            "Balance breakdown".to_string(),
            format!("{:.2} cash + {:.2} voucher {}", cash, voucher, currency),
        ));
    }
    if lines.is_empty() {
        anyhow::bail!("no balance fields in response");
    }
    Ok(BalanceSnapshot {
        lines,
        amount,
        limits: Vec::new(),
    })
}

fn number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deepseek() -> &'static OpenAiCompatibleProfile {
        balance_profile("deepseek").unwrap()
    }

    fn moonshot() -> &'static OpenAiCompatibleProfile {
        balance_profile("moonshotai").unwrap()
    }

    fn opencode_go() -> &'static OpenAiCompatibleProfile {
        balance_profile("opencode-go").unwrap()
    }

    #[test]
    fn deepseek_snapshot_keeps_usage_rows_and_exposes_the_amount() {
        let snapshot = deepseek_snapshot_from_json(&json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "USD",
                "total_balance": "28.11",
                "granted_balance": "21.77",
                "topped_up_balance": "6.34"
            }]
        }))
        .unwrap();
        // `/usage` renders these rows verbatim.
        assert_eq!(
            snapshot.lines,
            vec![
                (
                    "Key status".to_string(),
                    "valid (balance available)".to_string()
                ),
                (
                    "Balance".to_string(),
                    "28.11 USD (6.34 paid + 21.77 granted)".to_string()
                ),
            ]
        );
        let chip = BalanceChip {
            provider: deepseek().display_name.to_string(),
            currency: snapshot.amount.as_ref().unwrap().currency.clone(),
            amount: snapshot.amount.as_ref().unwrap().amount,
        };
        assert_eq!(chip.label(), "DeepSeek $28.11");
        assert!(!chip.is_low());
        assert!(!chip.is_exhausted());
    }

    #[test]
    fn numeric_and_string_balances_both_produce_an_amount() {
        let from_string = deepseek_snapshot_from_json(&json!({
            "balance_infos": [{"currency": "USD", "total_balance": "0.00"}]
        }))
        .unwrap();
        assert_eq!(from_string.amount.unwrap().amount, 0.0);

        let from_number = deepseek_snapshot_from_json(&json!({
            "balance_infos": [{"currency": "USD", "total_balance": 2.5}]
        }))
        .unwrap();
        assert!(from_number.amount.unwrap().amount < 5.0);
    }

    #[test]
    fn missing_or_unparsable_balance_is_an_error_not_zero() {
        // Key status alone still renders, exactly as `/usage` always has, but
        // it carries no amount for the chip.
        let status_only = deepseek_snapshot_from_json(&json!({"is_available": true})).unwrap();
        assert!(status_only.amount.is_none());
        assert_eq!(status_only.lines.len(), 1);
        assert!(deepseek_snapshot_from_json(&json!({})).is_err());
        let unparsable = deepseek_snapshot_from_json(&json!({
            "balance_infos": [{"currency": "USD", "total_balance": "n/a"}]
        }))
        .unwrap();
        assert!(unparsable.amount.is_none());
        assert_eq!(unparsable.lines.len(), 1);
        assert!(moonshot_snapshot_from_json(&json!({"data": {}}), "x").is_err());
    }

    #[test]
    fn moonshot_snapshot_matches_usage_rows_and_endpoint_currency() {
        let snapshot = moonshot_snapshot_from_json(
            &json!({"data": {"available_balance": 120.0, "cash_balance": 100.0, "voucher_balance": 20.0}}),
            "https://api.moonshot.cn/v1",
        )
        .unwrap();
        assert_eq!(
            snapshot.lines,
            vec![
                ("Balance".to_string(), "120.00 CNY available".to_string()),
                (
                    "Key status".to_string(),
                    "valid (balance available)".to_string()
                ),
                (
                    "Balance breakdown".to_string(),
                    "100.00 cash + 20.00 voucher CNY".to_string()
                ),
            ]
        );
        let amount = snapshot.amount.unwrap();
        assert_eq!(format_amount(&amount.currency, amount.amount), "¥120.00");
    }

    #[test]
    fn opencode_go_snapshot_parses_windows_and_resets() {
        // Shape observed from GET https://opencode.ai/zen/go/v1/usage.
        let snapshot = opencode_go_snapshot_from_json(&json!({
            "usage": {
                "rolling": {"status": "ok", "percent": 2, "resetsAt": "2026-10-29T16:30:36.000Z"},
                "weekly": {"status": "ok", "percent": 20, "resetsAt": "2026-11-02T00:00:00.000Z"},
                "monthly": {"status": "ok", "percent": 31, "resetsAt": "2026-11-14T11:06:07.000Z"}
            }
        }))
        .unwrap();
        assert!(snapshot.amount.is_none());
        assert_eq!(
            snapshot
                .limits
                .iter()
                .map(|limit| (limit.name.as_str(), limit.usage_percent))
                .collect::<Vec<_>>(),
            vec![("5-hour", 2.0), ("Weekly", 20.0), ("Monthly", 31.0)]
        );
        assert_eq!(
            snapshot.limits[0].resets_at.as_deref(),
            Some("2026-10-29T16:30:36.000Z")
        );
        assert_eq!(
            snapshot.limits[2].resets_at.as_deref(),
            Some("2026-11-14T11:06:07.000Z")
        );

        let partial = opencode_go_snapshot_from_json(&json!({
            "usage": {"rolling": {"percent": 105}}
        }))
        .unwrap();
        assert_eq!(partial.limits.len(), 1);
        assert_eq!(partial.limits[0].usage_percent, 100.0);
        assert!(partial.limits[0].resets_at.is_none());

        // Numeric strings and millisecond epochs are accepted in case the
        // backend changes encoding; both are normalized before reaching the UI.
        let alternate = opencode_go_snapshot_from_json(&json!({
            "usage": {"rolling": {"percent": "12", "resets_at": 1792835722000i64}}
        }))
        .unwrap();
        assert_eq!(alternate.limits[0].usage_percent, 12.0);
        assert_eq!(
            alternate.limits[0].resets_at.as_deref(),
            Some("2026-10-24T09:55:22+00:00")
        );
    }

    #[test]
    fn opencode_go_snapshot_rejects_missing_windows() {
        assert!(opencode_go_snapshot_from_json(&json!({})).is_err());
        assert!(opencode_go_snapshot_from_json(&json!({"usage": {}})).is_err());
        assert!(
            opencode_go_snapshot_from_json(&json!({"usage": {"rolling": {"status": "ok"}}}))
                .is_err()
        );
    }

    #[test]
    fn provider_keys_and_labels_resolve_only_to_balance_profiles() {
        assert_eq!(
            balance_profile_id_from_key("openai-compatible:deepseek"),
            Some("deepseek")
        );
        assert_eq!(
            balance_profile_id_from_key("deepseek@work"),
            Some("deepseek")
        );
        assert_eq!(balance_profile_id_from_key("openrouter"), None);
        assert_eq!(balance_profile_id_from_key("openai-compatible:groq"), None);
        assert_eq!(
            balance_profile_id_from_key("openai-compatible:opencode-go"),
            Some("opencode-go")
        );
        assert_eq!(
            balance_profile_for_label("DeepSeek").map(|profile| profile.id),
            Some("deepseek")
        );
        assert_eq!(balance_profile_for_label("Groq"), None);
        assert_eq!(
            balance_profile_for_label("OpenCode Go").map(|profile| profile.id),
            Some("opencode-go")
        );
        // OpenCode Zen shares the host but exposes no usage endpoint.
        assert_eq!(balance_profile("opencode"), None);
        assert_eq!(balance_profile_for_label("OpenCode Zen"), None);
        assert_eq!(balance_profile("kimi"), None);
        assert_eq!(moonshot().id, "moonshotai");
        assert_eq!(opencode_go().id, "opencode-go");
    }

    #[tokio::test]
    async fn providers_without_a_balance_endpoint_are_silent() {
        assert!(fetch_chip("openrouter").await.unwrap().is_none());
    }

    /// Manual gate: needs a live key, so it stays out of the default run.
    /// `cargo test -p jcode-base --lib usage::balance::tests::live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "hits the live DeepSeek balance endpoint"]
    async fn live_deepseek_balance_round_trip() {
        let chip = match fetch_chip("deepseek")
            .await
            .expect("balance request should not fail")
            .expect("a configured DeepSeek key should report a balance")
        {
            ProviderChip::Balance(chip) => chip,
            other => panic!("expected a balance chip, got {other:?}"),
        };
        println!("live balance: {}", chip.label());
        assert!(chip.amount >= 0.0);
    }

    /// Manual gate: needs a live OpenCode Go key.
    /// `cargo test -p jcode-base --lib usage::balance::tests::live_opencode -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "hits the unofficial OpenCode Go usage endpoint"]
    async fn live_opencode_go_usage_round_trip() {
        let chip = match fetch_chip("opencode-go")
            .await
            .expect("usage request should not fail")
            .expect("a configured OpenCode Go key should report usage")
        {
            ProviderChip::Limits(chip) => chip,
            other => panic!("expected a limits chip, got {other:?}"),
        };
        println!(
            "live OpenCode Go windows: {:?}",
            chip.windows
                .iter()
                .map(|window| (&window.name, window.usage_percent, &window.resets_at))
                .collect::<Vec<_>>()
        );
        assert_eq!(chip.windows.len(), 3);
        assert!(chip.max_usage_percent() <= 100.0);
    }
}
