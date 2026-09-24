use super::*;

#[test]
fn ciede2000_published_34_pairs() {
    let mut count = 0;
    for line in include_str!("ciede2000.txt").lines() {
        let values: Vec<f64> = line
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if values.len() != 7 {
            continue;
        }
        let actual = de(
            [values[0], values[1], values[2]],
            [values[3], values[4], values[5]],
        );
        assert!(
            (actual - values[6]).abs() < 0.00005,
            "pair {count}: {actual} != {}",
            values[6]
        );
        count += 1;
    }
    assert_eq!(count, 34);
}

#[test]
fn colour_references_and_quantized_gamut() {
    assert!((contrast(Rgb([0, 0, 0]), Rgb([255, 255, 255])) - 21.).abs() < 1e-9);
    let red = Rgb([255, 0, 0]);
    let lab = red.lab();
    assert!((lab[0] - 53.2408).abs() < 0.001);
    let [l, c, h] = red.oklch();
    assert_eq!(gamut(l, c, h), red);
    for h in 0..360 {
        let mapped = gamut(0.5, 0.4, f64::from(h)).oklch();
        assert!((mapped[0] - 0.5).abs() < 0.002);
        assert!(hue_gap(mapped[2], f64::from(h)) < 2.);
    }
}

#[test]
fn every_theme_has_readable_unique_rendered_candidates() {
    for (theme, profile) in THEMES.iter().enumerate() {
        let bank = bank::generate(theme, 40, 1024);
        assert!(bank.len() >= 64, "{}: {}", profile.name, bank.len());
        let mut seen = std::collections::HashSet::new();
        for c in bank {
            assert!(contrast(c.header, bank::INK).max(contrast(c.header, bank::LIGHT)) >= 4.5);
            assert!(seen.insert(c.header));
        }
    }
}

#[test]
fn no_op_preserves_rng_and_exact_checkpoint_continuation() {
    let mut w = Workspace::new(0, 137, &[]).expect("bank");
    let ids = (0..16).map(|i| format!("tab-{i}")).collect::<Vec<_>>();
    w.reconcile(&ids);
    let saved = serde_json::to_vec(&w).expect("serialize");
    w.reconcile(&ids);
    assert_eq!(serde_json::to_vec(&w).expect("serialize"), saved);
    let mut restored: Workspace = serde_json::from_slice(&saved).expect("restore");
    assert!(restored.restore());
    let mut next = ids.clone();
    next.rotate_left(3);
    w.reconcile(&next);
    restored.reconcile(&next);
    next.pop();
    w.reconcile(&next);
    restored.reconcile(&next);
    next.push("fresh".into());
    w.reconcile(&next);
    restored.reconcile(&next);
    assert_eq!(
        serde_json::to_vec(&w).expect("serialize"),
        serde_json::to_vec(&restored).expect("serialize")
    );
    assert!(w.history.len() <= 16);
}

#[test]
fn repair_budget_and_displacement_are_action_relative() {
    for theme in [0, 1, 10, 23] {
        let mut w = Workspace::new(theme, 77, &[]).expect("bank");
        let mut ids = (0..16).map(|i| format!("tab-{i}")).collect::<Vec<_>>();
        w.reconcile(&ids);
        for _ in 0..12 {
            let before = w
                .tabs
                .iter()
                .map(|t| (t.id.clone(), t.colour))
                .collect::<HashMap<_, _>>();
            let allocation = w.allocation.0;
            // Batch permutation: no uniquely identifiable direct-move tab allowance.
            ids.rotate_left(4);
            w.reconcile(&ids);
            let changed = w
                .tabs
                .iter()
                .filter(|t| before[&t.id] != t.colour)
                .collect::<Vec<_>>();
            assert!(changed.len() <= 2);
            for t in changed {
                assert!(w.bank[t.colour].within(w.bank[before[&t.id]]));
            }
            assert_eq!(allocation, w.allocation.0);
        }
    }
}

#[test]
fn rendering_tint_preserves_explicit_background_and_source() {
    let mut source = FrameData::from_ratatui_buffer_with_hyperlinks(
        &Buffer::empty(Rect::new(0, 0, 2, 1)),
        None,
        &[],
    );
    source.cells[1].bg = crate::protocol::color_to_u32(Rgb([12, 34, 56]).color());
    let original = source.clone();
    let mut target = source.clone();
    let tint = Some(0x02112233);
    super::super::blit_pane_surface_tinted(&mut target, &source, Rect::new(0, 0, 2, 1), tint);
    let mut patch = source.cells.clone();
    for c in &mut patch {
        tint_default(c, tint);
    }
    assert_eq!(target.cells, patch);
    assert_eq!(source, original);
    assert_eq!(target.cells[0].bg, 0x02112233);
    assert_eq!(target.cells[1].bg, source.cells[1].bg);
}

#[test]
fn feasible_reuse_beats_a_fresh_conflict() {
    let mut w = Workspace::new(1, 91, &[]).expect("bank");
    let a = w.bank[0];
    let b = *w
        .bank
        .iter()
        .find(|b| a.gap(**b) >= 1.)
        .expect("distinct pair");
    w.bank = vec![a, b];
    w.parent = 0;
    w.tabs = vec![
        allocator::Tab {
            id: "a".into(),
            colour: 0,
        },
        allocator::Tab {
            id: "b".into(),
            colour: 0,
        },
    ];
    w.history.push_back(1);
    w.reconcile(&["a".into(), "new".into(), "b".into()]);
    assert_eq!(w.tabs[1].colour, 1);
    assert!(w.conflicts().is_empty());
}

#[test]
fn world_lifecycle_stable_parent_and_destination_family() {
    let mut snapshot = super::super::tests::snapshot();
    let mut world = World {
        rng: Rng(15),
        workspaces: HashMap::new(),
    };
    assert!(world.reconcile(&snapshot));
    let first = serde_json::to_value(&world).expect("serialize");
    snapshot.workspaces[0].label = "renamed".into();
    snapshot.tabs[0].focused = false;
    snapshot.revision += 1;
    assert!(!world.reconcile(&snapshot));
    assert_eq!(serde_json::to_value(&world).expect("serialize"), first);
    let parent = world.workspaces["ws_1"].bank[world.workspaces["ws_1"].parent].header;
    let mut second = snapshot.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    snapshot.workspaces.push(second);
    world.reconcile(&snapshot);
    assert_eq!(
        world.workspaces["ws_1"].bank[world.workspaces["ws_1"].parent].header,
        parent
    );
    assert_ne!(
        world.workspaces["ws_1"].theme,
        world.workspaces["ws_2"].theme
    );
    snapshot.tabs[0].workspace_id = "ws_2".into();
    world.reconcile(&snapshot);
    assert!(world.workspaces["ws_1"].tabs.is_empty());
    assert_eq!(world.workspaces["ws_2"].tabs[0].id, "tab_1");
    let seed = world.workspaces["ws_2"].seed;
    snapshot.tabs.clear();
    world.reconcile(&snapshot);
    assert_eq!(world.workspaces["ws_2"].seed, seed);
}

#[test]
fn reported_host_colours_guard_surfaces_and_unknown_host_stays_untinted() {
    use crate::raw_input::RawInputEvent;
    use crate::terminal_theme::{DefaultColorKind, RgbColor};
    let snapshot = super::super::tests::snapshot();
    let endpoint = ClientEndpointId::Local;
    let mut colours = Colours::new(None);
    colours.reconcile(&endpoint, &snapshot);
    assert!(colours.surface(&endpoint, &snapshot).is_none());
    let bg = RgbColor {
        r: 24,
        g: 27,
        b: 32,
    };
    let fg = RgbColor {
        r: 214,
        g: 217,
        b: 224,
    };
    colours.update_host(&RawInputEvent::HostDefaultColor {
        kind: DefaultColorKind::Background,
        color: bg,
    });
    colours.update_host(&RawInputEvent::HostDefaultColor {
        kind: DefaultColorKind::Foreground,
        color: fg,
    });
    let surface = colours.surfaces["local"]["ws_1"];
    assert!(contrast(Rgb([fg.r, fg.g, fg.b]), surface) >= 4.5);
    let host_white = RgbColor {
        r: 250,
        g: 250,
        b: 250,
    };
    colours.update_host(&RawInputEvent::HostDefaultColor {
        kind: DefaultColorKind::Background,
        color: host_white,
    });
    colours.update_host(&RawInputEvent::HostDefaultColor {
        kind: DefaultColorKind::Foreground,
        color: bg,
    });
    assert!(contrast(Rgb([bg.r, bg.g, bg.b]), colours.surfaces["local"]["ws_1"]) >= 4.5);
}

#[test]
#[ignore = "supporting latency evidence; not a wall-clock CI gate"]
fn workspace_colours_profile() {
    use std::time::Instant;
    for theme in [0, 1, 10, 23] {
        let start = Instant::now();
        let mut w = Workspace::new(theme, 91, &[]).expect("bank");
        let bank_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut ids = Vec::new();
        let mut timings = Vec::new();
        for i in 0..16 {
            ids.push(format!("tab-{i}"));
            let start = Instant::now();
            w.reconcile(&ids);
            timings.push(start.elapsed().as_secs_f64() * 1000.);
        }
        let mut moves = Vec::new();
        for i in 0..30 {
            let tab = ids.remove(i % 16);
            ids.insert((i * 7) % 16, tab);
            let start = Instant::now();
            w.reconcile(&ids);
            moves.push(start.elapsed().as_secs_f64() * 1000.);
        }
        timings.sort_by(f64::total_cmp);
        moves.sort_by(f64::total_cmp);
        let start = Instant::now();
        for _ in 0..1000 {
            w.reconcile(&ids);
        }
        eprintln!("{}: bank={bank_ms:.2}ms add median={:.2} max={:.2}ms move median={:.2} max={:.2}ms noop={:.2}us remaining={}",THEMES[theme].name,timings[8],timings[15],moves[15],moves[29],start.elapsed().as_secs_f64()*1000.,w.remaining_conflicts);
    }
}

#[test]
fn sidebar_text_is_readable_and_selection_is_restrained() {
    for palette in [Palette::catppuccin(), Palette::catppuccin_latte()] {
        let mut colours = Colours::new(None);
        let mut world = World::default();
        for theme in 0..THEMES.len() {
            world.workspaces.insert(
                format!("w{theme}"),
                Workspace::new(theme, 42, &[]).expect("bank"),
            );
        }
        colours.worlds.insert("local".into(), world);
        colours.set_sidebar_theme(&palette);
        assert_eq!(colours.sidebar_styles.len(), THEMES.len());
        for style in colours
            .sidebar_styles
            .values()
            .chain(colours.tab_styles.values())
        {
            for fg in [style.title, style.detail] {
                assert!(contrast(fg, style.background) >= 4.5);
                assert!(contrast(fg, style.selected) >= 4.5);
            }
            assert!(style.selected.oklch()[1] < 0.06);
        }
    }
}
