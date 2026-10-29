//! Nonblocking provider credit / plan-quota chip rendered at the status
//! line's right edge.
//!
//! The chip is UI-only telemetry: a failed query or a provider without a
//! balance API never blocks a turn and never renders a fabricated balance.
//! Only the provider currently in use is polled, once a minute.

use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use crate::usage::{BalanceChip, LimitChip, ProviderChip};

const POLL_INTERVAL: Duration = Duration::from_secs(60);
const RETRY_INTERVAL: Duration = Duration::from_secs(120);
const FETCH_DEADLINE: Duration = Duration::from_secs(10);

/// Presentation state for the chip; keeps the render path free of formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub label: String,
    /// Shorter labels tried in order when the full one would collide with the
    /// left-hand status text. Balance chips carry none.
    pub fallbacks: Vec<String>,
    pub low: bool,
    pub exhausted: bool,
}

impl Chip {
    fn from_balance(balance: BalanceChip) -> Self {
        Self {
            label: balance.label(),
            fallbacks: Vec::new(),
            low: balance.is_low(),
            exhausted: balance.is_exhausted(),
        }
    }

    fn from_limits(limits: LimitChip) -> Self {
        let low = limits.is_low();
        let exhausted = limits.is_exhausted();
        Self {
            label: limits_label(&limits, true),
            fallbacks: vec![limits_label(&limits, false)],
            low,
            exhausted,
        }
    }

    fn from_provider(chip: ProviderChip) -> Self {
        match chip {
            ProviderChip::Balance(balance) => Self::from_balance(balance),
            ProviderChip::Limits(limits) => Self::from_limits(limits),
        }
    }
}

/// Compact label for a rotating window name. The `/usage` overlay keeps the
/// long form; the status line gets the shortest unambiguous one.
fn window_short_name(name: &str) -> &str {
    match name {
        "5-hour" => "5h",
        "Weekly" => "1w",
        "Monthly" => "1mo",
        other => other,
    }
}

/// `OpenCode Go 5h 2% (58m) · 1w 20% (3d 6h) · 1mo 31% (15d)`.
fn limits_label(limits: &LimitChip, include_resets: bool) -> String {
    let windows = limits
        .windows
        .iter()
        .map(|window| {
            let reset = if include_resets {
                window
                    .resets_at
                    .as_deref()
                    .map(jcode_tui_usage_overlay::format_reset_time)
                    .filter(|reset| !reset.is_empty())
                    .map(|reset| format!(" ({reset})"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            format!(
                "{} {:.0}%{}",
                window_short_name(&window.name),
                window.usage_percent,
                reset
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    if windows.is_empty() {
        limits.provider.clone()
    } else {
        format!("{} {}", limits.provider, windows)
    }
}

#[derive(Default)]
pub(crate) struct Poller {
    profile: Option<String>,
    pending: Option<oneshot::Receiver<Option<ProviderChip>>>,
    chip: Option<Chip>,
    next_poll: Option<Instant>,
}

impl Poller {
    pub(crate) fn chip(&self) -> Option<&Chip> {
        self.chip.as_ref()
    }

    /// Never waits on the network on the UI thread. Returns whether the
    /// rendered chip changed so the caller can request a redraw.
    pub(crate) fn refresh(&mut self, profile: Option<&str>, enabled: bool) -> bool {
        let previous = self.chip.clone();
        let Some(profile) = profile.filter(|_| enabled) else {
            self.profile = None;
            self.pending = None;
            self.chip = None;
            self.next_poll = None;
            return self.chip != previous;
        };

        // Switching provider retires the old number immediately instead of
        // showing one provider's credit next to another provider's traffic.
        if self.profile.as_deref() != Some(profile) {
            self.profile = Some(profile.to_string());
            self.pending = None;
            self.chip = None;
            self.next_poll = Some(Instant::now());
        }

        let now = Instant::now();
        if let Some(receiver) = &mut self.pending {
            match receiver.try_recv() {
                Ok(balance) => {
                    self.chip = balance.map(Chip::from_provider);
                    self.pending = None;
                    self.next_poll = Some(
                        now + if self.chip.is_some() {
                            POLL_INTERVAL
                        } else {
                            RETRY_INTERVAL
                        },
                    );
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.chip = None;
                    self.pending = None;
                    self.next_poll = Some(now + RETRY_INTERVAL);
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if self.pending.is_none()
            && self.next_poll.is_none_or(|next| now >= next)
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let profile = profile.to_string();
            let (sender, receiver) = oneshot::channel();
            self.pending = Some(receiver);
            runtime.spawn(async move {
                let balance =
                    match tokio::time::timeout(FETCH_DEADLINE, crate::usage::fetch_chip(&profile))
                        .await
                    {
                        Ok(Ok(balance)) => balance,
                        Ok(Err(error)) => {
                            crate::logging::debug(&format!(
                                "Provider balance unavailable: {error:#}"
                            ));
                            None
                        }
                        Err(_) => {
                            crate::logging::debug("Provider balance query timed out");
                            None
                        }
                    };
                if sender.send(balance).is_err() {
                    crate::logging::debug("Discarded provider balance after poller was closed");
                }
            });
        }

        self.chip != previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(amount: f64, currency: &str) -> BalanceChip {
        BalanceChip {
            provider: "DeepSeek".to_string(),
            currency: currency.to_string(),
            amount,
        }
    }

    fn provider_chip(amount: f64, currency: &str) -> ProviderChip {
        ProviderChip::Balance(chip(amount, currency))
    }

    fn limits_chip(windows: &[(&str, f32, Option<&str>)]) -> LimitChip {
        LimitChip {
            provider: "OpenCode Go".to_string(),
            windows: windows
                .iter()
                .map(|(name, percent, resets_at)| crate::usage::UsageLimit {
                    name: name.to_string(),
                    usage_percent: *percent,
                    resets_at: resets_at.map(|value| value.to_string()),
                })
                .collect(),
        }
    }

    #[test]
    fn chip_reports_low_and_exhausted_balances() {
        let healthy = Chip::from_balance(chip(28.11, "USD"));
        assert_eq!(healthy.label, "DeepSeek $28.11");
        assert!(!healthy.low && !healthy.exhausted);

        let low = Chip::from_balance(chip(2.5, "USD"));
        assert!(low.low && !low.exhausted);

        let empty = Chip::from_balance(chip(0.0, "USD"));
        assert!(empty.low && empty.exhausted);
    }

    #[test]
    fn quota_chip_lists_every_window_with_its_reset() {
        let chip = Chip::from_limits(limits_chip(&[
            ("5-hour", 2.0, None),
            ("Weekly", 20.0, None),
            ("Monthly", 31.0, None),
        ]));
        assert_eq!(chip.label, "OpenCode Go 5h 2% · 1w 20% · 1mo 31%");
        assert_eq!(chip.fallbacks, vec![chip.label.clone()]);
        assert!(!chip.low && !chip.exhausted);

        let chip = Chip::from_limits(limits_chip(&[
            ("5-hour", 2.0, Some("2099-01-01T00:00:00Z")),
            ("Weekly", 20.0, Some("2099-01-03T00:00:00Z")),
            ("Monthly", 31.0, Some("2099-02-01T00:00:00Z")),
        ]));
        assert!(chip.label.starts_with("OpenCode Go 5h 2% ("));
        assert!(chip.label.contains(" · 1w 20% ("));
        assert!(chip.label.contains(" · 1mo 31% ("));
        assert_eq!(
            chip.fallbacks,
            vec!["OpenCode Go 5h 2% · 1w 20% · 1mo 31%".to_string()]
        );
    }

    #[test]
    fn quota_chip_warns_at_seventy_and_exhausts_at_one_hundred() {
        let warned = Chip::from_limits(limits_chip(&[("Weekly", 70.0, None)]));
        assert!(warned.low && !warned.exhausted);
        let exhausted = Chip::from_limits(limits_chip(&[("Monthly", 100.0, None)]));
        assert!(exhausted.low && exhausted.exhausted);
    }

    #[test]
    fn disabling_or_switching_clears_the_chip() {
        let mut poller = Poller {
            profile: Some("deepseek".to_string()),
            chip: Some(Chip::from_balance(chip(28.11, "USD"))),
            ..Default::default()
        };
        assert!(poller.refresh(None, true));
        assert!(poller.chip().is_none());

        let mut poller = Poller::default();
        poller.refresh(Some("deepseek"), false);
        assert!(poller.chip().is_none());
        assert!(poller.profile.is_none());
        assert!(poller.pending.is_none());
    }

    #[test]
    fn provider_switch_drops_the_previous_balance_immediately() {
        let mut poller = Poller {
            profile: Some("deepseek".to_string()),
            chip: Some(Chip::from_balance(chip(28.11, "USD"))),
            ..Default::default()
        };
        // A stale in-flight query must not repopulate the chip for the new
        // provider.
        let (sender, receiver) = oneshot::channel();
        sender.send(Some(provider_chip(1.0, "USD"))).unwrap();
        poller.pending = Some(receiver);
        poller.refresh(Some("moonshotai"), true);
        assert!(poller.chip().is_none());
        assert_eq!(poller.profile.as_deref(), Some("moonshotai"));
        assert!(poller.pending.is_none());
    }

    #[test]
    fn completed_query_sets_the_chip_and_schedules_the_next_poll() {
        let mut poller = Poller {
            profile: Some("deepseek".to_string()),
            ..Default::default()
        };
        let (sender, receiver) = oneshot::channel();
        sender.send(Some(provider_chip(0.0, "USD"))).unwrap();
        poller.pending = Some(receiver);
        assert!(poller.refresh(Some("deepseek"), true));
        let chip = poller.chip().unwrap();
        assert!(chip.exhausted);
        assert!(poller.next_poll.unwrap() > Instant::now());
    }

    #[test]
    fn failed_query_renders_nothing_and_backs_off() {
        let mut poller = Poller {
            profile: Some("deepseek".to_string()),
            chip: Some(Chip::from_balance(chip(28.11, "USD"))),
            ..Default::default()
        };
        let (sender, receiver) = oneshot::channel();
        sender.send(None).unwrap();
        poller.pending = Some(receiver);
        assert!(poller.refresh(Some("deepseek"), true));
        assert!(poller.chip().is_none());
        let delay = poller
            .next_poll
            .unwrap()
            .saturating_duration_since(Instant::now());
        assert!(delay > POLL_INTERVAL);
    }
}
