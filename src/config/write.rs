#[derive(Clone, Copy)]
pub(crate) enum ConfigEdit<'a> {
    Theme(&'a str),
    WorkspaceColours(bool),
    WorkspaceColourPalette(super::WorkspaceColourPalette),
    StatusIndicators(super::StatusIndicatorStyle),
    Sound(bool),
    ToastDelivery(super::ToastDelivery),
}

impl ConfigEdit<'_> {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Theme(_) => "theme",
            Self::WorkspaceColours(_) | Self::WorkspaceColourPalette(_) => "workspace colours",
            Self::StatusIndicators(_) => "status indicators",
            Self::Sound(_) => "sound setting",
            Self::ToastDelivery(_) => "toast setting",
        }
    }

    pub(crate) fn apply(self, content: &str) -> String {
        match self {
            Self::WorkspaceColourPalette(mode) => {
                let value = match mode {
                    super::WorkspaceColourPalette::Theme => "\"theme\"",
                    super::WorkspaceColourPalette::Mixed => "\"mixed\"",
                };
                let content = super::upsert_section_value(
                    content,
                    "theme",
                    "workspace_colour_palette",
                    value,
                );
                super::upsert_section_bool(&content, "theme", "workspace_colours", true)
            }
            Self::WorkspaceColours(enabled) => {
                super::upsert_section_bool(content, "theme", "workspace_colours", enabled)
            }
            Self::Theme(name) => {
                let content =
                    super::upsert_section_value(content, "theme", "name", &format!("\"{name}\""));
                super::upsert_section_bool(&content, "theme", "auto_switch", false)
            }
            Self::StatusIndicators(style) => super::upsert_section_value(
                content,
                "ui",
                "status_indicators",
                &format!("\"{}\"", style.as_str()),
            ),
            Self::Sound(enabled) => {
                super::upsert_section_bool(content, "ui.sound", "enabled", enabled)
            }
            Self::ToastDelivery(delivery) => {
                let value = match delivery {
                    super::ToastDelivery::Off => "\"off\"",
                    super::ToastDelivery::Herdr => "\"herdr\"",
                    super::ToastDelivery::Terminal => "\"terminal\"",
                    super::ToastDelivery::System => "\"system\"",
                };
                let content = super::upsert_section_value(content, "ui.toast", "delivery", value);
                super::remove_section_key(&content, "ui.toast", "enabled")
            }
        }
    }
}

pub(crate) fn update_file_at(
    path: &std::path::Path,
    description: &str,
    update: impl FnOnce(&str) -> String,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create config directory: {error}"))?;
    }
    let content = match super::io::read_optional_config(path) {
        Ok(Some(content)) => content,
        Ok(None) => String::new(),
        Err(error) => {
            return Err(format!(
                "failed to read config before saving {description}: {error}"
            ));
        }
    };
    std::fs::write(path, update(&content))
        .map_err(|error| format!("failed to save {description}: {error}"))
}

pub(crate) fn write_edit(edit: ConfigEdit<'_>) -> Result<(), String> {
    update_file_at(&super::config_path(), edit.description(), |content| {
        edit.apply(content)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_colours_edit_preserves_theme_and_other_settings() {
        let original = "# my theme\n[theme]\nname = \"nord\"\nauto_switch = true\n\n[ui]\nmouse_capture = false\n";
        let enabled = ConfigEdit::WorkspaceColours(true).apply(original);
        let disabled = ConfigEdit::WorkspaceColours(false).apply(&enabled);
        for (content, expected) in [(&enabled, true), (&disabled, false)] {
            let config: crate::config::Config = toml::from_str(content).expect("valid config");
            assert_eq!(config.theme.workspace_colours, expected);
            assert_eq!(config.theme.name.as_deref(), Some("nord"));
            assert!(config.theme.auto_switch);
            assert!(!config.ui.mouse_capture);
            assert!(content.starts_with("# my theme\n"));
            assert_eq!(content.matches("workspace_colours =").count(), 1);
        }
        let fresh = ConfigEdit::WorkspaceColours(true).apply("");
        let config: crate::config::Config = toml::from_str(&fresh).expect("new config");
        assert!(config.theme.workspace_colours);
    }

    #[test]
    fn update_file_at_does_not_move_a_leading_bom_into_the_file() {
        let dir = std::env::temp_dir().join(format!("herdr-config-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            b"\xEF\xBB\xBF[terminal]\ndefault_shell = \"pwsh.exe\"\n",
        )
        .unwrap();

        update_file_at(&path, "onboarding setting", |content| {
            crate::config::upsert_top_level_bool(content, "onboarding", false)
        })
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        assert!(
            !written.contains('\u{feff}'),
            "unexpected BOM in {written:?}"
        );
        assert!(
            toml::from_str::<toml::Value>(&written).is_ok(),
            "written config is not valid TOML: {written:?}"
        );
    }
}

#[cfg(test)]
mod workspace_palette_tests {
    use super::*;

    #[test]
    fn workspace_palette_mode_enables_colours_without_changing_base_theme() {
        let original =
            "[theme]\nname = \"gruvbox\"\nauto_switch = true\nworkspace_colours = false\n";
        for mode in [
            super::super::WorkspaceColourPalette::Theme,
            super::super::WorkspaceColourPalette::Mixed,
        ] {
            let written = ConfigEdit::WorkspaceColourPalette(mode).apply(original);
            let config: crate::config::Config = toml::from_str(&written).expect("config");
            assert!(config.theme.workspace_colours);
            assert_eq!(config.theme.workspace_colour_palette, mode);
            assert_eq!(config.theme.name.as_deref(), Some("gruvbox"));
            assert!(config.theme.auto_switch);
        }
        let existing: crate::config::Config =
            toml::from_str("[theme]\nworkspace_colours = true\n").expect("legacy config");
        assert_eq!(
            existing.theme.workspace_colour_palette,
            super::super::WorkspaceColourPalette::Mixed
        );
    }
}
