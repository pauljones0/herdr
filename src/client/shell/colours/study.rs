//! Manual study of the final displayed colours (not just allocator candidates).
use super::*;

#[test]
#[ignore = "manual palette study; writes evidence to HERDR_COLOUR_STUDY_DIR"]
fn workspace_colour_palette_study() {
    let directory =
        std::path::PathBuf::from(std::env::var("HERDR_COLOUR_STUDY_DIR").expect("study directory"));
    std::fs::create_dir_all(&directory).expect("study directory");
    let mut records = Vec::new();
    for name in [
        "catppuccin",
        "nord",
        "gruvbox",
        "rose-pine",
        "catppuccin-latte",
        "vesper",
        "dracula",
        "tokyo-night",
    ] {
        let palette = Palette::from_name(name).expect("built-in palette");
        let rgb = |c| {
            if let Color::Rgb(r, g, b) = c {
                Rgb([r, g, b])
            } else {
                panic!("RGB palette")
            }
        };
        for mode in [
            crate::config::WorkspaceColourPalette::Mixed,
            crate::config::WorkspaceColourPalette::Theme,
        ] {
            let tone = (mode == crate::config::WorkspaceColourPalette::Theme)
                .then(|| projection::ThemeTone::from_palette(&palette))
                .flatten();
            for (theme, profile) in THEMES.iter().enumerate() {
                for seed in [40, 77, 137] {
                    let mut w = Workspace::new(theme, seed, &[]).expect("family");
                    w.reconcile(&(0..8).map(|i| format!("t{i}")).collect::<Vec<_>>());
                    let style = |c| {
                        chrome_style(
                            c,
                            rgb(palette.panel_bg),
                            rgb(palette.active_row_bg),
                            true,
                            tone,
                        )
                    };
                    let tabs = w
                        .tabs
                        .iter()
                        .map(|t| style(w.bank[t.colour]))
                        .collect::<Vec<_>>();
                    let gaps = tabs
                        .windows(2)
                        .map(|p| de(p[0].detail.lab(), p[1].detail.lab()))
                        .collect::<Vec<_>>();
                    let min_contrast = tabs
                        .iter()
                        .flat_map(|s| {
                            [
                                contrast(s.title, s.background),
                                contrast(s.title, s.selected),
                                contrast(s.detail, s.background),
                                contrast(s.detail, s.selected),
                            ]
                        })
                        .fold(f64::INFINITY, f64::min);
                    let header = chrome_style(
                        w.bank[w.parent],
                        rgb(palette.panel_bg),
                        rgb(palette.active_row_bg),
                        false,
                        tone,
                    );
                    records.push(serde_json::json!({"base":name,"mode":format!("{mode:?}"),"family":profile.name,"seed":seed,"header":header.title.0,"tabs":tabs.iter().map(|s|s.detail.0).collect::<Vec<_>>(),"adjacent_de":gaps,"min_contrast":min_contrast}));
                }
            }
        }
    }
    std::fs::write(
        directory.join("measurements.json"),
        serde_json::to_vec_pretty(&records).expect("JSON"),
    )
    .expect("write measurements");
    // Optional live-session identity manifest: all IDs must come from the CLI, never guessed.
    if let Ok(bytes) = std::fs::read(directory.join("live-identities.json")) {
        let entries: Vec<(String, Vec<String>, usize)> =
            serde_json::from_slice(&bytes).expect("live identities");
        let mut world = World {
            rng: Rng(4242),
            workspaces: HashMap::new(),
        };
        for (i, (id, tabs, theme)) in entries.into_iter().enumerate() {
            let mut workspace = Workspace::new(theme, 77 + i as u64, &[]).expect("demo family");
            workspace.reconcile(&tabs);
            world.workspaces.insert(id, workspace);
        }
        let data = serde_json::json!({"version":1,"worlds":{"local":world}});
        std::fs::write(
            directory.join("demo-colours.json"),
            serde_json::to_vec_pretty(&data).expect("checkpoint"),
        )
        .expect("demo checkpoint");
    }
}
