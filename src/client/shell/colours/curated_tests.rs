use super::*;

fn value(world: &World) -> serde_json::Value {
    serde_json::to_value(world).expect("world")
}

#[test]
fn curated_profile_roundtrip_and_preview_preserve_original() {
    let snapshot = super::super::tests::snapshot();
    let endpoint = ClientEndpointId::Local;
    let mut colours = Colours::new(None);
    colours.reconcile(&endpoint, &snapshot);
    let original = value(colours.worlds.values().next().expect("original"));
    colours.select_profile(Some("nord"));
    colours.reconcile(&endpoint, &snapshot);
    let first = value(colours.active_worlds().values().next().expect("nord"));
    colours.begin_preview();
    colours.select_profile(Some("dracula"));
    colours.reconcile(&endpoint, &snapshot);
    colours.finish_preview(false);
    colours.select_profile(Some("nord"));
    colours.reconcile(&endpoint, &snapshot);
    assert_eq!(
        value(colours.active_worlds().values().next().expect("nord")),
        first
    );
    assert_eq!(
        value(colours.worlds.values().next().expect("original")),
        original
    );
}

#[test]
fn curated_sets_cover_rgb_builtins_and_exclude_terminal() {
    for name in crate::config::THEME_NAMES {
        if *name == "terminal" {
            assert!(curated::families(name).is_none());
            continue;
        }
        let families = curated::families(name).expect("curated RGB theme");
        assert!(families.len() >= 6 && families.len() < THEMES.len());
        let mut unique = families.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), families.len());
        assert!(families.iter().all(|&i| i < THEMES.len()));
    }
}

#[test]
fn curated_membership_overflow_and_replacement_are_deterministic() {
    let mut snapshot = super::super::tests::snapshot();
    let base = snapshot.workspaces[0].clone();
    let mut original = World {
        rng: Rng(10),
        workspaces: HashMap::new(),
    };
    snapshot.workspaces.clear();
    snapshot.tabs.clear();
    for i in 0..15 {
        let id = format!("workspace-{i:02}");
        let mut row = base.clone();
        row.workspace_id = id.clone();
        snapshot.workspaces.push(row);
        original
            .workspaces
            .insert(id, Workspace::new(23, 50 + i, &[]).expect("bank"));
    }
    let mut a = World {
        rng: Rng(0),
        workspaces: HashMap::new(),
    };
    a.reconcile_curated(&original, "nord", &snapshot);
    let reversed = World {
        rng: Rng(10),
        workspaces: original
            .workspaces
            .iter()
            .map(|(id, w)| (id.clone(), w.clone()))
            .collect(),
    };
    let mut b = World {
        rng: Rng(0),
        workspaces: HashMap::new(),
    };
    b.reconcile_curated(&reversed, "nord", &snapshot);
    assert_eq!(value(&a), value(&b));
    let allowed = curated::families("nord").expect("set");
    assert_eq!(a.workspaces.len(), 15);
    assert!(a.workspaces.values().all(|w| allowed.contains(&w.theme)));
    for family in allowed {
        assert!(a.workspaces.values().filter(|w| &w.theme == family).count() <= 3);
    }
}

#[test]
fn curated_compatible_allocations_and_live_topology_survive_cancel() {
    let mut snapshot = super::super::tests::snapshot();
    let endpoint = ClientEndpointId::Local;
    let mut colours = Colours::new(None);
    colours.reconcile(&endpoint, &snapshot);
    let key = endpoint.storage_key();
    let mut compatible = Workspace::new(7, 1234, &[]).expect("glacier");
    compatible.reconcile(&["tab_1".into()]);
    colours
        .worlds
        .get_mut(&key)
        .expect("world")
        .workspaces
        .insert("ws_1".into(), compatible.clone());
    colours.begin_preview();
    colours.select_profile(Some("nord"));
    colours.reconcile(&endpoint, &snapshot);
    assert_eq!(
        serde_json::to_value(colours.workspace(&endpoint, "ws_1")).expect("preview"),
        serde_json::to_value(&compatible).expect("original")
    );
    let mut tab = snapshot.tabs[0].clone();
    tab.tab_id = "new-tab".into();
    snapshot.tabs.push(tab);
    colours.reconcile(&endpoint, &snapshot);
    colours.finish_preview(false);
    colours.select_profile(None);
    assert_eq!(
        colours
            .workspace(&endpoint, "ws_1")
            .expect("original")
            .tabs
            .len(),
        2
    );
    assert!(colours.curated_worlds.is_empty());
}

#[test]
fn curated_checkpoint_restart_and_cancel_do_not_touch_original_file() {
    let directory = std::env::temp_dir().join(format!("herdr-curated-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("directory");
    let path = directory.join("prefs.json");
    let endpoint = ClientEndpointId::Local;
    let snapshot = super::super::tests::snapshot();
    let expected;
    {
        let mut colours = Colours::new(Some(&path));
        colours.reconcile(&endpoint, &snapshot);
        colours.select_profile(Some("nord"));
        colours.reconcile(&endpoint, &snapshot);
        expected = serde_json::to_value(colours.workspace(&endpoint, "ws_1")).expect("assignment");
    }
    let original_bytes =
        std::fs::read(path.with_extension("colours-v1.json")).expect("original checkpoint");
    let curated_bytes =
        std::fs::read(path.with_extension("colours-curated-v1.json")).expect("curated checkpoint");
    {
        let mut colours = Colours::new(Some(&path));
        colours.select_profile(Some("nord"));
        colours.reconcile(&endpoint, &snapshot);
        assert_eq!(
            serde_json::to_value(colours.workspace(&endpoint, "ws_1")).expect("restored"),
            expected
        );
        colours.begin_preview();
        colours.select_profile(Some("dracula"));
        colours.reconcile(&endpoint, &snapshot);
        colours.finish_preview(false);
    }
    assert_eq!(
        std::fs::read(path.with_extension("colours-v1.json")).expect("original"),
        original_bytes
    );
    assert_eq!(
        std::fs::read(path.with_extension("colours-curated-v1.json")).expect("curated"),
        curated_bytes
    );
    std::fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn cancelled_fresh_preview_does_not_create_either_checkpoint() {
    let directory =
        std::env::temp_dir().join(format!("herdr-curated-fresh-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("directory");
    let path = directory.join("prefs.json");
    {
        let mut colours = Colours::new(Some(&path));
        colours.begin_preview();
        colours.select_profile(Some("nord"));
        colours.reconcile(&ClientEndpointId::Local, &super::super::tests::snapshot());
        colours.finish_preview(false);
        colours.select_profile(None);
    }
    assert!(!path.with_extension("colours-v1.json").exists());
    assert!(!path.with_extension("colours-curated-v1.json").exists());
    std::fs::remove_dir_all(directory).expect("cleanup");
}
