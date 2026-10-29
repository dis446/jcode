//! Provider credit balances, shared by `/usage` and the live status chip.
//!
//! `/usage` lists every connected provider and renders one row per balance
//! field; the status chip only needs the remaining amount for the provider in
//! use. Both read the same response through the same fetch and parse helpers
//! here, so the chip can never disagree with `/usage` about a balance.

use anyhow::{Context, Result};
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
    matches!(profile_id, "deepseek" | "moonshotai")
}

/// Fetch the live balance for one profile. `Ok(None)` means the provider has
/// no balance API or no configured key, which is not an error.
pub async fn fetch_balance(profile_id: &str) -> Result<Option<BalanceChip>> {
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
        _ => return Ok(None),
    };
    let Some(amount) = snapshot.amount else {
        anyhow::bail!("no numeric balance in response");
    };
    Ok(Some(BalanceChip {
        provider: profile.display_name.to_string(),
        currency: amount.currency,
        amount: amount.amount,
    }))
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
    Ok(BalanceSnapshot { lines, amount })
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
    Ok(BalanceSnapshot { lines, amount })
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
                ("Key status".to_string(), "valid (balance available)".to_string()),
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
                (
                    "Balance".to_string(),
                    "120.00 CNY available".to_string()
                ),
                ("Key status".to_string(), "valid (balance available)".to_string()),
                (
                    "Balance breakdown".to_string(),
                    "100.00 cash + 20.00 voucher CNY".to_string()
                ),
            ]
        );
        let amount = snapshot.amount.unwrap();
        assert_eq!(
            format_amount(&amount.currency, amount.amount),
            "¥120.00"
        );
    }

    #[test]
    fn provider_keys_and_labels_resolve_only_to_balance_profiles() {
        assert_eq!(
            balance_profile_id_from_key("openai-compatible:deepseek"),
            Some("deepseek")
        );
        assert_eq!(balance_profile_id_from_key("deepseek@work"), Some("deepseek"));
        assert_eq!(balance_profile_id_from_key("openrouter"), None);
        assert_eq!(balance_profile_id_from_key("openai-compatible:groq"), None);
        assert_eq!(
            balance_profile_for_label("DeepSeek").map(|profile| profile.id),
            Some("deepseek")
        );
        assert_eq!(balance_profile_for_label("Groq"), None);
        assert_eq!(balance_profile("kimi"), None);
        assert_eq!(moonshot().id, "moonshotai");
    }

    #[tokio::test]
    async fn providers_without_a_balance_endpoint_are_silent() {
        assert!(fetch_balance("openrouter").await.unwrap().is_none());
    }

    /// Manual gate: needs a live key, so it stays out of the default run.
    /// `cargo test -p jcode-base --lib usage::balance::tests::live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "hits the live DeepSeek balance endpoint"]
    async fn live_deepseek_balance_round_trip() {
        let chip = fetch_balance("deepseek")
            .await
            .expect("balance request should not fail")
            .expect("a configured DeepSeek key should report a balance");
        println!("live balance: {}", chip.label());
        assert!(chip.amount >= 0.0);
    }
}
