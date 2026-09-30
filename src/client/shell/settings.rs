use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

// The Theme section starts with the off/theme/mixed choices for workspace colours.
pub(super) const THEME_CHOICE_OFFSET: usize = 3;

pub(super) fn normalized_theme_name(name: &str) -> String {
    crate::config::canonical_theme_name(name)
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_lowercase().replace([' ', '_'], "-"))
}

fn theme_index(name: &str) -> usize {
    let normalized = normalized_theme_name(name);
    crate::config::THEME_NAMES
        .iter()
        .position(|candidate| normalized_theme_name(candidate) == normalized)
        .unwrap_or(0)
        + THEME_CHOICE_OFFSET
}

fn indicator_index(style: crate::config::StatusIndicatorStyle) -> usize {
    usize::from(style == crate::config::StatusIndicatorStyle::Symbols)
}

fn toast_index(delivery: crate::config::ToastDelivery) -> usize {
    match delivery {
        crate::config::ToastDelivery::Off => 0,
        crate::config::ToastDelivery::Herdr => 1,
        crate::config::ToastDelivery::Terminal => 2,
        crate::config::ToastDelivery::System => 3,
    }
}

pub(super) fn integration_needs_install(info: &crate::api::schema::IntegrationInfo) -> bool {
    info.state == crate::api::schema::IntegrationState::Outdated
        || info.available && info.state == crate::api::schema::IntegrationState::NotInstalled
}

impl ClientShellState {
    pub(super) fn open_settings_overlay(&mut self) {
        self.colours
            .enable_persistence(self.config.preferences_path.as_deref());
        self.colours.begin_preview();
        self.overlay = Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
            section: ClientSettingsSection::Theme,
            selected: theme_index(&self.config.theme_name),
            original_theme_name: self.config.theme_name.clone(),
            original_workspace_colours: self.config.workspace_colours,
            original_workspace_colour_palette: self.config.workspace_colour_palette,
            original_palette: self.config.palette.clone(),
            draft_theme_name: None,
            draft_workspace_colours: self.config.workspace_colours,
            draft_workspace_colour_palette: self.config.workspace_colour_palette,
            palette_notice: String::new(),
            preview_theme_name: String::new(),
            integrations: Vec::new(),
            integration_messages: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
        }));
        self.refresh_theme_presentation();
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.selected = theme_index(&settings.preview_theme_name);
        }
    }

    fn selected_index_for_settings_section(&self, section: ClientSettingsSection) -> usize {
        match section {
            ClientSettingsSection::Theme => {
                let name = match &self.overlay {
                    Some(ClientShellOverlay::Settings(settings)) => &settings.preview_theme_name,
                    _ => &self.config.theme_name,
                };
                theme_index(name)
            }
            ClientSettingsSection::Indicators => indicator_index(self.config.status_indicators),
            ClientSettingsSection::Sound => usize::from(!self.config.sound_enabled),
            ClientSettingsSection::Toast => toast_index(self.config.toast_delivery),
            ClientSettingsSection::Integrations => 0,
        }
    }

    pub(super) fn select_settings_section(
        &mut self,
        section: ClientSettingsSection,
        outcome: &mut ClientShellInput,
    ) {
        let selected = self.selected_index_for_settings_section(section);
        let request_integrations = matches!(section, ClientSettingsSection::Integrations)
            && matches!(
                self.overlay,
                Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                    loading_integrations: false,
                    installing_integrations: false,
                    ..
                }))
            );
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.section = section;
            settings.selected = selected;
        }
        if request_integrations {
            self.queue_integration_list(outcome, true);
        }
        outcome.repaint = true;
    }

    fn move_settings_section(&mut self, delta: isize, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() else {
            return;
        };
        let current = ClientSettingsSection::ALL
            .iter()
            .position(|section| *section == settings.section)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(ClientSettingsSection::ALL.len() as isize)
            as usize;
        self.select_settings_section(ClientSettingsSection::ALL[next], outcome);
    }

    fn settings_choice_count(&self) -> usize {
        match self.overlay.as_ref() {
            Some(ClientShellOverlay::Settings(settings)) => match settings.section {
                ClientSettingsSection::Theme => {
                    THEME_CHOICE_OFFSET + crate::config::THEME_NAMES.len()
                }
                ClientSettingsSection::Indicators | ClientSettingsSection::Sound => 2,
                ClientSettingsSection::Toast => 4,
                ClientSettingsSection::Integrations => settings.integrations.len(),
            },
            _ => 0,
        }
    }

    pub(super) fn move_settings_selection(&mut self, delta: isize) {
        let count = self.settings_choice_count();
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        if count == 0 {
            settings.selected = 0;
            return;
        }
        let (first, last) = if settings.section == ClientSettingsSection::Theme {
            if settings.selected < THEME_CHOICE_OFFSET {
                (0, THEME_CHOICE_OFFSET - 1)
            } else {
                (THEME_CHOICE_OFFSET, count - 1)
            }
        } else {
            (0, count - 1)
        };
        settings.selected =
            (settings.selected as isize + delta).clamp(first as isize, last as isize) as usize;
        if settings.section == ClientSettingsSection::Theme {
            self.preview_selected_theme();
        }
    }

    pub(super) fn select_settings_choice(&mut self, index: usize) {
        let count = self.settings_choice_count();
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            if count > 0 {
                settings.selected = index.min(count - 1);
            }
        }
        if matches!(
            self.overlay,
            Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                section: ClientSettingsSection::Theme,
                ..
            }))
        ) {
            self.preview_selected_theme();
        }
    }

    fn preview_selected_theme(&mut self) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        if let Some(index) = settings.selected.checked_sub(THEME_CHOICE_OFFSET) {
            if let Some(name) = crate::config::THEME_NAMES.get(index) {
                settings.draft_theme_name = Some((*name).to_owned());
            }
        } else {
            settings.draft_workspace_colours = settings.selected != 0;
            if settings.selected != 0 {
                settings.draft_workspace_colour_palette = if settings.selected == 1 {
                    crate::config::WorkspaceColourPalette::Theme
                } else {
                    crate::config::WorkspaceColourPalette::Mixed
                };
            }
        }
        self.refresh_theme_presentation();
    }

    // Event-time resolution shared by settings, config reload, and host appearance.
    // Never infer an automatic light/dark profile from the saved manual name.
    pub(super) fn refresh_theme_presentation(&mut self) {
        let (mut palette, mut name) =
            crate::app::resolve_effective_theme(&self.config.theme_runtime, self.host_appearance);
        self.config.theme_name = self.config.theme_runtime.manual_name.clone();
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() {
            if let Some(draft) = &settings.draft_theme_name {
                name = draft.clone();
                palette = crate::app::client_palette_for_theme(&self.config.theme_runtime, draft);
                self.config.theme_name = draft.clone();
            }
            self.config.workspace_colours = settings.draft_workspace_colours;
            self.config.workspace_colour_palette = settings.draft_workspace_colour_palette;
        }
        self.config.palette = palette;
        self.update_curated_profile(&name);
    }

    pub(super) fn update_curated_profile(&mut self, name: &str) {
        let canonical = crate::config::canonical_theme_name(name);
        let profile = canonical.filter(|name| {
            colours::curated_families(name).is_some()
                && Palette::from_name(name).as_ref() == Some(&self.config.palette)
        });
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.preview_theme_name = name.to_owned();
            settings.palette_notice = if !settings.draft_workspace_colours {
                "Standard Herdr styling.".into()
            } else if settings.draft_workspace_colour_palette
                == crate::config::WorkspaceColourPalette::Mixed
            {
                "Full catalogue; related colours within each workspace.".into()
            } else if profile.is_some() {
                "Curated families; changing theme may change workspace colours.".into()
            } else {
                "Uncurated custom/terminal theme: using Original palettes.".into()
            };
        }
        let adapted = self.config.workspace_colours
            && self.config.workspace_colour_palette == crate::config::WorkspaceColourPalette::Theme;
        self.colours
            .select_profile(if adapted { profile } else { None });
        if self.config.workspace_colours {
            if let Some(snapshot) = self.snapshot.as_deref() {
                self.colours.reconcile(&self.active_endpoint_id, snapshot);
            }
            for endpoint in &self.endpoints {
                if endpoint.endpoint_id != self.active_endpoint_id {
                    if let Some(snapshot) = endpoint.snapshot.as_deref() {
                        self.colours.reconcile(&endpoint.endpoint_id, snapshot);
                    }
                }
            }
        }
    }

    pub(super) fn cancel_settings_overlay(&mut self) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.take() else {
            return;
        };
        self.config.theme_name = settings.original_theme_name;
        self.config.palette = settings.original_palette;
        self.config.workspace_colours = settings.original_workspace_colours;
        self.config.workspace_colour_palette = settings.original_workspace_colour_palette;
        self.colours.finish_preview(false);
        self.refresh_theme_presentation();
    }

    fn save_settings_edit(
        &mut self,
        edit: crate::config::ConfigEdit<'_>,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if let Err(error) = crate::config::write_edit(edit) {
            self.set_endpoint_error(error);
            outcome.repaint = true;
            return false;
        }
        self.reload_client_config();
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::ServerReloadConfig(
                crate::api::schema::EmptyParams::default(),
            ),
            PendingEndpointKind::ReloadConfig,
            outcome,
        );
        outcome.repaint = true;
        true
    }

    pub(super) fn apply_settings_choice(&mut self, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() else {
            return;
        };
        let section = settings.section;
        let selected = settings.selected;
        match section {
            ClientSettingsSection::Theme => {
                let theme = settings.draft_theme_name.clone();
                let edit = crate::config::ConfigEdit::Appearance {
                    theme: theme.as_deref(),
                    enabled: settings.draft_workspace_colours,
                    palette: settings.draft_workspace_colour_palette,
                };
                if self.save_settings_edit(edit, outcome) {
                    self.colours.finish_preview(true);
                    self.overlay = None;
                    self.refresh_theme_presentation();
                }
            }
            ClientSettingsSection::Indicators => {
                let style = if selected == 0 {
                    crate::config::StatusIndicatorStyle::Dots
                } else {
                    crate::config::StatusIndicatorStyle::Symbols
                };
                self.save_settings_edit(
                    crate::config::ConfigEdit::StatusIndicators(style),
                    outcome,
                );
            }
            ClientSettingsSection::Sound => {
                self.save_settings_edit(crate::config::ConfigEdit::Sound(selected == 0), outcome);
            }
            ClientSettingsSection::Toast => {
                let delivery = match selected {
                    0 => crate::config::ToastDelivery::Off,
                    1 => crate::config::ToastDelivery::Herdr,
                    2 => crate::config::ToastDelivery::Terminal,
                    _ => crate::config::ToastDelivery::System,
                };
                self.save_settings_edit(
                    crate::config::ConfigEdit::ToastDelivery(delivery),
                    outcome,
                );
            }
            ClientSettingsSection::Integrations => self.install_recommended_integrations(outcome),
        }
    }

    fn queue_integration_list(&mut self, outcome: &mut ClientShellInput, clear_messages: bool) {
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.loading_integrations = true;
            if clear_messages {
                settings.integration_messages.clear();
            }
        }
        if !self.push_endpoint_method_with_kind(
            crate::api::schema::Method::IntegrationList(crate::api::schema::EmptyParams::default()),
            PendingEndpointKind::IntegrationList,
            outcome,
        ) {
            if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                settings.loading_integrations = false;
            }
        }
    }

    fn install_recommended_integrations(&mut self, outcome: &mut ClientShellInput) {
        if self.pending_integration_installs > 0 {
            return;
        }
        let targets = match self.overlay.as_ref() {
            Some(ClientShellOverlay::Settings(settings)) => settings
                .integrations
                .iter()
                .filter(|integration| integration_needs_install(integration))
                .map(|integration| integration.target)
                .collect::<Vec<_>>(),
            _ => return,
        };
        if targets.is_empty() {
            return;
        }
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.installing_integrations = true;
            settings.integration_messages.clear();
        }
        self.pending_integration_installs = 0;
        for target in targets {
            if self.push_endpoint_method_with_kind(
                crate::api::schema::Method::IntegrationInstall(
                    crate::api::schema::IntegrationInstallParams { target },
                ),
                PendingEndpointKind::IntegrationInstall,
                outcome,
            ) {
                self.pending_integration_installs += 1;
            }
        }
        if self.pending_integration_installs == 0 {
            if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                settings.installing_integrations = false;
            }
        }
        outcome.repaint = true;
    }

    pub(super) fn handle_settings_endpoint_result(
        &mut self,
        kind: PendingEndpointKind,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> (bool, Vec<ClientShellAction>) {
        match kind {
            PendingEndpointKind::IntegrationList => {
                if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                    settings.loading_integrations = false;
                    match result {
                        Ok(crate::api::schema::ResponseResult::IntegrationList {
                            integrations,
                        }) => {
                            settings.integrations = integrations;
                            settings.selected = settings
                                .selected
                                .min(settings.integrations.len().saturating_sub(1));
                        }
                        Ok(_) => {
                            self.set_endpoint_error(
                                "endpoint returned an unexpected integration list result",
                            );
                        }
                        Err(_) => {}
                    }
                }
                (true, Vec::new())
            }
            PendingEndpointKind::IntegrationInstall => {
                let cancelled = result
                    .as_ref()
                    .is_err_and(|error| error.code.as_deref() == Some("endpoint_cancelled"));
                self.pending_integration_installs =
                    self.pending_integration_installs.saturating_sub(1);
                if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                    match result {
                        Ok(crate::api::schema::ResponseResult::IntegrationInstall {
                            details,
                            ..
                        }) => settings.integration_messages.extend(details.messages),
                        Ok(_) => settings
                            .integration_messages
                            .push("endpoint returned an unexpected integration result".into()),
                        Err(error) => settings.integration_messages.push(error.message),
                    }
                    settings.installing_integrations = self.pending_integration_installs > 0;
                }
                let actions = if !cancelled
                    && self.pending_integration_installs == 0
                    && matches!(self.overlay, Some(ClientShellOverlay::Settings(_)))
                {
                    let mut deferred = ClientShellInput::default();
                    self.queue_integration_list(&mut deferred, false);
                    deferred.actions
                } else {
                    Vec::new()
                };
                (true, actions)
            }
            _ => (false, Vec::new()),
        }
    }

    pub(super) fn route_settings_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !matches!(self.overlay, Some(ClientShellOverlay::Settings(_))) {
            return false;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        if code == KeyCode::Esc {
            if !matches!(
                self.overlay,
                Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                    installing_integrations: true,
                    ..
                }))
            ) {
                self.cancel_settings_overlay();
                outcome.repaint = true;
            }
            return true;
        }
        if matches!(code, KeyCode::Tab | KeyCode::BackTab)
            && modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                if settings.section == ClientSettingsSection::Theme {
                    settings.selected = if settings.selected < THEME_CHOICE_OFFSET {
                        theme_index(&settings.preview_theme_name)
                    } else if !settings.draft_workspace_colours {
                        0
                    } else if settings.draft_workspace_colour_palette
                        == crate::config::WorkspaceColourPalette::Theme
                    {
                        1
                    } else {
                        2
                    };
                    outcome.repaint = true;
                    return true;
                }
            }
        }
        if matches!(code, KeyCode::Tab | KeyCode::Right | KeyCode::Char('l'))
            && modifiers.is_empty()
        {
            self.move_settings_section(1, outcome);
            return true;
        }
        if matches!(code, KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h'))
            && modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            self.move_settings_section(-1, outcome);
            return true;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && modifiers.is_empty() {
            self.move_settings_selection(-1);
            outcome.repaint = true;
            return true;
        }
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && modifiers.is_empty() {
            self.move_settings_selection(1);
            outcome.repaint = true;
            return true;
        }
        if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) && modifiers.is_empty() {
            self.apply_settings_choice(outcome);
            return true;
        }
        true
    }
}
