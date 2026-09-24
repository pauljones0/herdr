use super::*;

fn themed_state() -> ClientShellState {
    let mut config = Config::default();
    config.theme.workspace_colours = true;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.handle_raw_events(vec![
        RawInputEvent::HostDefaultColor {
            kind: crate::terminal_theme::DefaultColorKind::Background,
            color: crate::terminal_theme::RgbColor {
                r: 24,
                g: 27,
                b: 32,
            },
        },
        RawInputEvent::HostDefaultColor {
            kind: crate::terminal_theme::DefaultColorKind::Foreground,
            color: crate::terminal_theme::RgbColor {
                r: 214,
                g: 217,
                b: 224,
            },
        },
    ]);
    state
}

#[test]
fn workspace_colours_keep_fast_patches_equivalent_to_full_composition() {
    let mut state = themed_state();
    let frame = state.compose(100, 30).expect("frame");
    let mut pane = state.pane_surface.as_ref().unwrap().panes[0].clone();
    pane.content_revision = 2;
    let explicit = 0x02ff1122;
    let cells = [0, explicit, 0, 0]
        .map(|bg| crate::protocol::CellData {
            symbol: "N".into(),
            fg: 0,
            bg,
            modifier: crate::protocol::modifier_to_u16(
                Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED,
            ),
            skip: false,
            hyperlink: None,
        })
        .to_vec();
    let ClientPaneSurfacePatchOutcome::Applied(Some(patch)) =
        state.apply_pane_surface_patch(crate::protocol::PaneSurfacePatch {
            boot_id: "boot-1".into(),
            projection_revision: 1,
            base_surface_revision: 1,
            surface_revision: 2,
            rows: vec![crate::protocol::PaneSurfacePatchRow { x: 0, y: 0, cells }],
            panes: vec![pane],
            cursor: None,
        })
    else {
        panic!("tints must preserve the fast patch path");
    };
    let patched = apply_composed_surface_patch(&frame, patch).expect("patched");
    let full = state.compose(100, 30).expect("full");
    assert_eq!(patched.cells, full.cells);
    let raw = &state.pane_surface.as_ref().unwrap().frame.cells;
    assert_eq!(raw[0].bg, 0);
    assert_eq!(raw[1].bg, explicit);
}

#[test]
fn workspace_colours_focus_uses_bold_without_extra_decorations() {
    let mut state = themed_state();
    let mut next = snapshot();
    let mut tab = next.tabs[0].clone();
    tab.tab_id = "tab_2".into();
    tab.label = "second".into();
    tab.focused = false;
    next.tabs.push(tab);
    state.set_snapshot(Box::new(next));
    let frame = state
        .compose(100, 30)
        .expect("frame")
        .to_ratatui_buffer()
        .expect("buffer");
    assert_eq!(state.hits.tabs.len(), 2);
    for (i, (rect, _)) in state.hits.tabs.iter().enumerate() {
        assert!(!frame[(rect.x + 1, rect.y)]
            .modifier
            .contains(Modifier::UNDERLINED));
        assert_eq!(
            frame[(rect.x + 1, rect.y)]
                .modifier
                .contains(Modifier::BOLD),
            i == 0
        );
        assert_eq!(frame[(rect.x, rect.y)].symbol(), " ");
        assert_eq!(frame[(rect.x, rect.y)].bg, frame[(rect.x + 1, rect.y)].bg);
        if i == 0 {
            assert_ne!(frame[(rect.x, rect.y)].bg, state.config.palette.panel_bg);
        } else {
            assert_eq!(frame[(rect.x, rect.y)].bg, state.config.palette.panel_bg);
        }
        assert!(!frame[(rect.x + 1, rect.y)].modifier.contains(Modifier::DIM));
    }
}

#[test]
#[ignore = "non-gating fixed-geometry client composition profile"]
fn workspace_colours_render_scale_profile() {
    for count in [1, 15] {
        for (enabled, mode) in [
            (false, crate::config::WorkspaceColourPalette::Mixed),
            (true, crate::config::WorkspaceColourPalette::Mixed),
            (true, crate::config::WorkspaceColourPalette::Theme),
        ] {
            let mut state = themed_state();
            state.config.workspace_colours = enabled;
            state.config.workspace_colour_palette = mode;
            let mut next = surface();
            next.surface_revision += 1;
            next.frame = FrameData::from_ratatui_buffer(
                &Buffer::with_lines(vec!["x".repeat(120); 30]),
                None,
            );
            let template = next.panes[0].clone();
            next.panes.clear();
            for i in 0..count {
                let mut pane = template.clone();
                pane.pane_id = format!("pane_{i}");
                pane.inner_rect = SurfaceRect {
                    x: if count == 1 { 0 } else { (i % 5) * 24 },
                    y: if count == 1 { 0 } else { (i / 5) * 10 },
                    width: if count == 1 { 120 } else { 24 },
                    height: if count == 1 { 30 } else { 10 },
                };
                pane.rect = pane.inner_rect;
                next.panes.push(pane);
            }
            state.set_pane_surface(next);
            let mut projected = snapshot();
            for i in 0..count {
                let mut agent = colour_test_agent();
                agent.pane_id = format!("pane_{i}");
                agent.focused = i == 0;
                projected.agents.push(agent);
            }
            state.set_snapshot(Box::new(projected));
            state.compose(146, 32).expect("frame");
            let mut samples = Vec::new();
            for _ in 0..400 {
                let start = std::time::Instant::now();
                std::hint::black_box(state.compose(146, 32).expect("frame"));
                samples.push(start.elapsed().as_micros());
            }
            samples.sort_unstable();
            eprintln!(
                "colours panes={count} enabled={enabled} mode={mode:?} median={}us p95={}us",
                samples[200], samples[380]
            );
        }
    }
}

#[test]
fn workspace_colours_agents_match_owning_tabs_across_endpoints() {
    let mut state = themed_state();
    let mut next = snapshot();
    next.agents.push(colour_test_agent());
    state.set_snapshot(Box::new(next.clone()));
    let remote_id = ClientEndpointId::Ssh(
        crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef")
            .expect("profile ID"),
    );
    let mut remote = state.endpoints[0].clone();
    remote.endpoint_id = remote_id.clone();
    remote.snapshot = Some(Box::new(next.clone()));
    state.colours.reconcile(&remote_id, &next);
    state
        .colours
        .set_sidebar_theme(&state.config.palette, state.config.workspace_colour_palette);
    let palette = &state.config.palette;
    for endpoint in [ClientEndpointId::Local, remote_id] {
        let family = state
            .colours
            .agent_style(&endpoint, "ws_1", "tab_1")
            .expect("owning tab style");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 5));
        let row =
            super::super::agent_sidebar::agent_row(&next, "pane_1", &state.config, None, None)
                .expect("agent");
        super::super::agent_sidebar::render_agent_row(
            &mut buffer,
            Rect::new(0, 0, 30, 5),
            &row,
            &state.config,
            Some(family),
        );
        assert!(buffer.content.iter().any(|c| c.fg == family.foreground(0)));
        assert!(buffer
            .content
            .iter()
            .any(|c| c.fg == palette.yellow && c.symbol() != " "));
        assert!(!buffer
            .content
            .iter()
            .any(|c| c.modifier.contains(Modifier::UNDERLINED)));
    }
}

fn colour_test_agent() -> crate::protocol::ClientShellAgent {
    crate::protocol::ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some("reviewer".into()),
        display_agent: None,
        agent: Some("codex".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: crate::api::schema::AgentStatus::Working,
        state_change_seq: 1,
        state_labels: vec![],
        tokens: vec![],
        focused: true,
    }
}

#[test]
fn workspace_colours_sidebar_neutralizes_host_and_tints_workspace_details() {
    let mut state = themed_state();
    for host in [[48, 10, 36], [255, 255, 255]] {
        let expected = state.config.palette.panel_bg;
        state.handle_raw_events(vec![RawInputEvent::HostDefaultColor {
            kind: crate::terminal_theme::DefaultColorKind::Background,
            color: crate::terminal_theme::RgbColor {
                r: host[0],
                g: host[1],
                b: host[2],
            },
        }]);
        let frame = state
            .compose(100, 30)
            .expect("frame")
            .to_ratatui_buffer()
            .expect("buffer");
        let sidebar = state.layout(100, 30).sidebar;
        assert_eq!(frame[(sidebar.x, sidebar.bottom() - 2)].bg, expected);
        let rect = state.hits.workspaces[0].rect;
        assert!(rect.height > 1, "fixture must include a detail row");
        let detail = &frame[(rect.x + 3, rect.y + 1)];
        assert_ne!(detail.bg, expected, "selected row keeps a subtle highlight");
        assert_ne!(detail.fg, state.config.palette.mauve);
        assert_ne!(detail.fg, state.config.palette.overlay0);
    }
}

#[test]
fn workspace_colours_respect_explicit_sidebar_token_styles_and_done_status() {
    let mut config: Config = toml::from_str(
        r##"
[theme]
workspace_colours = true
[ui.sidebar.spaces]
rows = [["state_icon", { token = "workspace", fg = "#123456", bold = false, dim = true }]]
[ui.sidebar.agents]
rows = [["state_icon", { token = "agent", fg = "#654321", bold = false, dim = true }]]
"##,
    )
    .expect("config");
    config.theme.workspace_colours = true;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut next = snapshot();
    next.workspaces[0].label = "ZZZZ".into();
    next.workspaces[0].agent_status = crate::api::schema::AgentStatus::Done;
    let mut agent = colour_test_agent();
    agent.name = Some("QQQQ".into());
    agent.agent_status = crate::api::schema::AgentStatus::Done;
    next.agents.push(agent);
    state.set_snapshot(Box::new(next));
    state.set_pane_surface(surface());
    // Focused-agent notification projection acknowledges Done. Set this renderer
    // fixture afterwards so the test exercises the Done colour role directly.
    state.snapshot.as_mut().expect("snapshot").workspaces[0].agent_status =
        crate::api::schema::AgentStatus::Done;
    let buffer = state
        .compose(106, 30)
        .expect("frame")
        .to_ratatui_buffer()
        .expect("buffer");
    for (symbol, expected) in [
        ("Z", ratatui::style::Color::Rgb(0x12, 0x34, 0x56)),
        ("Q", ratatui::style::Color::Rgb(0x65, 0x43, 0x21)),
    ] {
        let cell = buffer
            .content
            .iter()
            .find(|c| c.symbol() == symbol)
            .expect("custom label");
        assert_eq!(cell.fg, expected, "explicit token foreground wins");
        assert!(!cell.modifier.contains(Modifier::BOLD));
        assert!(cell.modifier.contains(Modifier::DIM));
    }
    let rect = state.hits.workspaces[0].rect;
    let status = (rect.x..rect.right())
        .map(|x| &buffer[(x, rect.y)])
        .find(|c| {
            c.symbol()
                == status_icon(
                    crate::api::schema::AgentStatus::Done,
                    config.ui.status_indicators,
                )
        })
        .expect("done icon");
    assert_eq!(status.fg, state.config.palette.teal);
}
