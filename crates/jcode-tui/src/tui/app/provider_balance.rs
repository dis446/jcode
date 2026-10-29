use super::App;

impl App {
    pub(super) fn refresh_provider_balance(&mut self) -> bool {
        let profile = self.provider_balance_profile();
        // SSH clients share the remote host's credentials, so a laptop-side
        // query would read the wrong key (or none) and show the wrong account.
        self.provider_balance.refresh(
            profile.as_deref(),
            crate::config::config().display.provider_balance && !crate::tui::is_ssh_remote(),
        )
    }

    /// Provider only when it has a balance endpoint, so the chip never claims
    /// "not available" for providers we simply cannot meter.
    fn provider_balance_profile(&self) -> Option<String> {
        if let Some(key) = self.session.provider_key.as_deref()
            && let Some(id) = crate::usage::balance_profile_id_from_key(key)
        {
            return Some(id.to_string());
        }
        if let Some(id) = crate::usage::balance_profile_id_from_key(self.provider.name()) {
            return Some(id.to_string());
        }
        let label = if self.is_remote {
            self.remote_provider_name.clone().unwrap_or_default()
        } else {
            self.provider.display_name()
        };
        crate::usage::balance_profile_for_label(&label).map(|profile| profile.id.to_string())
    }
}
