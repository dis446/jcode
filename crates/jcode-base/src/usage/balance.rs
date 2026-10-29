//! Compact provider credit balance for live UI surfaces.
//!
//! `/usage` reports the full picture for every connected provider. A status
//! chip only needs one number for the provider currently in use, so this
//! module queries that provider's balance endpoint directly and reports
//! "not available" (rather than zero) for providers that have no such API.

use anyhow::{Context, Result};
use std::time::Duration;

use crate::provider_catalog::{self, OpenAiCompatibleProfile};

const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

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

type BalanceParser =
    fn(&serde_json::Value, &OpenAiCompatibleProfile) -> Result<BalanceChip>;

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
    let client = crate::provider::shared_http_client();
    let resolved = provider_catalog::resolve_openai_compatible_profile(*profile);
    let base = resolved.api_base.trim_end_matches('/');

    let (url, parser): (String, BalanceParser) = match profile.id {
        "deepseek" => (
            "https://api.deepseek.com/user/balance".to_string(),
            parse_deepseek_balance,
        ),
        "moonshotai" => (format!("{base}/users/me/balance"), parse_moonshot_balance),
        _ => return Ok(None),
    };

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Accept", "application/json")
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("{} balance request failed", profile.display_name))?;
    if !response.status().is_success() {
        anyhow::bail!("{} balance HTTP {}", profile.display_name, response.status());
    }
    let json: serde_json::Value = response
        .json()
        .await
        .context("invalid balance response")?;
    parser(&json, profile).map(Some)
}

fn number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_deepseek_balance(
    json: &serde_json::Value,
    profile: &OpenAiCompatibleProfile,
) -> Result<BalanceChip> {
    let first = json
        .get("balance_infos")
        .and_then(|value| value.as_array())
        .and_then(|infos| infos.first())
        .ok_or_else(|| anyhow::anyhow!("no balance_infos in response"))?;
    let amount = first
        .get("total_balance")
        .and_then(number)
        .ok_or_else(|| anyhow::anyhow!("no numeric total_balance in response"))?;
    let currency = first
        .get("currency")
        .and_then(|value| value.as_str())
        .unwrap_or("USD");
    Ok(BalanceChip {
        provider: profile.display_name.to_string(),
        currency: currency.to_string(),
        amount,
    })
}

fn parse_moonshot_balance(
    json: &serde_json::Value,
    profile: &OpenAiCompatibleProfile,
) -> Result<BalanceChip> {
    let data = json
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("no balance data in response"))?;
    let amount = data
        .get("available_balance")
        .and_then(number)
        .ok_or_else(|| anyhow::anyhow!("no numeric available_balance in response"))?;
    // Moonshot reports the currency only through the account's endpoint, so
    // derive it from the API host the profile actually points at.
    let currency = if profile.api_base.contains("moonshot.cn") {
        "CNY"
    } else {
        "USD"
    };
    Ok(BalanceChip {
        provider: profile.display_name.to_string(),
        currency: currency.to_string(),
        amount,
    })
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
    fn deepseek_balance_uses_total_and_reports_currency() {
        let chip = parse_deepseek_balance(
            &json!({"is_available": true, "balance_infos": [{"currency": "USD", "total_balance": "28.11", "granted_balance": "21.77", "topped_up_balance": "6.34"}]}),
            deepseek(),
        )
        .unwrap();
        assert_eq!(chip.label(), "DeepSeek $28.11");
        assert!(!chip.is_low());
        assert!(!chip.is_exhausted());
    }

    #[test]
    fn numeric_string_and_number_balances_both_parse() {
        let from_string = parse_deepseek_balance(
            &json!({"balance_infos": [{"currency": "USD", "total_balance": "0.00"}]}),
            deepseek(),
        )
        .unwrap();
        assert!(from_string.is_exhausted());
        let from_number = parse_deepseek_balance(
            &json!({"balance_infos": [{"currency": "USD", "total_balance": 2.5}]}),
            deepseek(),
        )
        .unwrap();
        assert!(from_number.is_low());
    }

    #[test]
    fn malformed_or_missing_balance_is_an_error_not_zero() {
        assert!(parse_deepseek_balance(&json!({"is_available": true}), deepseek()).is_err());
        assert!(
            parse_deepseek_balance(
                &json!({"balance_infos": [{"currency": "USD", "total_balance": "n/a"}]}),
                deepseek()
            )
            .is_err()
        );
        assert!(parse_moonshot_balance(&json!({"data": {}}), moonshot()).is_err());
    }

    #[test]
    fn moonshot_balance_reports_the_endpoint_currency() {
        let mut profile = *moonshot();
        profile.api_base = "https://api.moonshot.cn/v1";
        let chip = parse_moonshot_balance(&json!({"data": {"available_balance": 120.0}}), &profile)
            .unwrap();
        assert_eq!(chip.label(), "Moonshot AI ¥120.00");
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
