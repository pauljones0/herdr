//! Curated catalogue subsets and client-local, per-theme identity assignments.
use super::*;

// Catalogue indices are persisted. Never reorder the catalogue to edit a set.
// Dark/light variants have separate checkpoint identities, even when the sets agree.
pub(in crate::client::shell) fn families(name: &str) -> Option<&'static [usize]> {
    Some(match name {
        "catppuccin" | "catppuccin-latte" => &[1, 8, 13, 18, 7, 19],
        "tokyo-night" | "tokyo-night-day" => &[15, 10, 14, 16, 20, 19],
        "dracula" => &[10, 16, 18, 14, 11, 9],
        "nord" => &[7, 12, 8, 0, 13, 19],
        "gruvbox" | "gruvbox-light" => &[0, 9, 11, 19, 14, 23],
        "one-dark" | "one-light" => &[3, 8, 0, 19, 4, 14],
        "solarized" | "solarized-light" => &[3, 14, 9, 11, 16, 15],
        "kanagawa" | "kanagawa-lotus" => &[15, 2, 0, 14, 19, 4],
        "rose-pine" | "rose-pine-dawn" => &[8, 13, 3, 7, 19, 2],
        "vesper" => &[19, 18, 14, 12, 4, 0],
        _ => return None,
    })
}

fn seed(name: &str, original: u64) -> u64 {
    name.bytes().fold(original ^ 0xcbf29ce484222325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

impl World {
    pub(super) fn reconcile_curated(
        &mut self,
        original: &World,
        profile: &str,
        snapshot: &ClientShellSnapshot,
    ) -> bool {
        let Some(allowed) = families(profile) else {
            return false;
        };
        // Agent-state-only snapshots must not allocate, sort, or regenerate colours.
        if self.workspaces.len() == original.workspaces.len()
            && self.workspaces.iter().all(|(id, w)| {
                original.workspaces.contains_key(id)
                    && allowed.contains(&w.theme)
                    && w.tabs.iter().map(|t| t.id.as_str()).eq(snapshot
                        .tabs
                        .iter()
                        .filter(|t| &t.workspace_id == id)
                        .map(|t| t.tab_id.as_str()))
            })
        {
            return false;
        }
        let before = self.workspaces.len();
        self.workspaces
            .retain(|id, w| original.workspaces.contains_key(id) && allowed.contains(&w.theme));
        let mut changed = before != self.workspaces.len();
        // Copy all compatible identities before assigning replacements. HashMap order
        // must never decide which workspace gets a family first.
        let mut ids = original.workspaces.keys().collect::<Vec<_>>();
        ids.sort();
        for id in &ids {
            let source = &original.workspaces[*id];
            if !self.workspaces.contains_key(*id) && allowed.contains(&source.theme) {
                self.workspaces.insert((*id).clone(), source.clone());
                changed = true;
            }
        }
        for id in ids {
            if !self.workspaces.contains_key(id) {
                let source = &original.workspaces[id];
                let choice_seed = seed(profile, source.seed);
                let mut choices = allowed.to_vec();
                // Spread families first, then break equal-occupancy ties stably.
                choices.sort_by_key(|&theme| {
                    (
                        self.workspaces
                            .values()
                            .filter(|w| w.theme == theme)
                            .count(),
                        seed(THEMES[theme].name, choice_seed),
                    )
                });
                let parents = self
                    .workspaces
                    .values()
                    .map(|w| w.bank[w.parent])
                    .collect::<Vec<_>>();
                if let Some(workspace) = Workspace::new(choices[0], choice_seed, &parents) {
                    self.workspaces.insert(id.clone(), workspace);
                    changed = true;
                }
            }
            if let Some(workspace) = self.workspaces.get_mut(id) {
                if !workspace.tabs.iter().map(|t| t.id.as_str()).eq(snapshot
                    .tabs
                    .iter()
                    .filter(|t| &t.workspace_id == id)
                    .map(|t| t.tab_id.as_str()))
                {
                    let tabs = snapshot
                        .tabs
                        .iter()
                        .filter(|t| &t.workspace_id == id)
                        .map(|t| t.tab_id.clone())
                        .collect::<Vec<_>>();
                    workspace.reconcile(&tabs);
                    changed = true;
                }
            }
        }
        changed
    }
}

impl Colours {
    pub(super) fn active_worlds(&self) -> &HashMap<String, World> {
        if self.active_profile.is_some() {
            self.preview.as_ref().unwrap_or(&self.curated_worlds)
        } else {
            &self.worlds
        }
    }

    pub(in crate::client::shell) fn select_profile(&mut self, profile: Option<&'static str>) {
        if self.active_profile == profile {
            return;
        }
        if let (Some(name), Some(preview)) = (profile, self.preview.as_mut()) {
            let prefix = format!("{name}/");
            for (key, world) in self
                .curated_worlds
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
            {
                preview.entry(key.clone()).or_insert_with(|| world.clone());
            }
        }
        if let Some(name) = profile {
            if let Some(allowed) = families(name) {
                let inherited = self
                    .endpoint_keys
                    .iter()
                    .filter_map(|(endpoint, key)| {
                        let world = self.active_worlds().get(self.render_keys.get(endpoint)?)?;
                        Some((
                            format!("{name}/{key}"),
                            World {
                                rng: Rng(0),
                                workspaces: world
                                    .workspaces
                                    .iter()
                                    .filter(|(_, w)| allowed.contains(&w.theme))
                                    .map(|(id, w)| (id.clone(), w.clone()))
                                    .collect(),
                            },
                        ))
                    })
                    .collect::<Vec<_>>();
                let worlds = self.preview.as_mut().unwrap_or(&mut self.curated_worlds);
                for (key, world) in inherited {
                    if let std::collections::hash_map::Entry::Vacant(entry) = worlds.entry(key) {
                        entry.insert(world);
                        self.curated_dirty = true;
                    }
                }
            }
        }
        self.active_profile = profile;
        self.render_keys = self
            .endpoint_keys
            .iter()
            .map(|(endpoint, key)| {
                (
                    endpoint.clone(),
                    profile.map_or_else(|| key.clone(), |p| format!("{p}/{key}")),
                )
            })
            .collect();
        self.refresh_surfaces();
    }

    pub(in crate::client::shell) fn begin_preview(&mut self) {
        let prefix = self.active_profile.map(|name| format!("{name}/"));
        self.preview = Some(
            self.curated_worlds
                .iter()
                .filter(|(key, _)| {
                    prefix
                        .as_ref()
                        .is_some_and(|prefix| key.starts_with(prefix))
                })
                .map(|(key, world)| (key.clone(), world.clone()))
                .collect(),
        );
    }

    pub(in crate::client::shell) fn finish_preview(&mut self, apply: bool) {
        if let Some(preview) = self.preview.take() {
            if apply {
                if self.original_dirty {
                    if let Some(writer) = &self.writer {
                        writer.save(&self.worlds);
                    }
                    self.original_dirty = false;
                }
                // Commit only the selected profile, not every set hovered during preview.
                if let Some(profile) = self.active_profile {
                    let prefix = format!("{profile}/");
                    self.curated_worlds.extend(
                        preview
                            .into_iter()
                            .filter(|(key, _)| key.starts_with(&prefix)),
                    );
                    if let Some(writer) = &self.curated_writer {
                        writer.save(&self.curated_worlds);
                    }
                }
            }
        }
        self.curated_dirty = false;
        self.refresh_surfaces();
    }

    pub(super) fn reconcile_curated(&mut self, key: &str, snapshot: &ClientShellSnapshot) -> bool {
        let Some(profile) = self.active_profile else {
            return false;
        };
        let Some(original) = self.worlds.get(key) else {
            return false;
        };
        let worlds = self.preview.as_mut().unwrap_or(&mut self.curated_worlds);
        let Some(render_key) = self.render_keys.values().find(|candidate| {
            candidate
                .strip_prefix(profile)
                .and_then(|rest| rest.strip_prefix('/'))
                == Some(key)
        }) else {
            return false;
        };
        if !worlds.contains_key(render_key) {
            worlds.insert(
                render_key.clone(),
                World {
                    rng: Rng(0),
                    workspaces: HashMap::new(),
                },
            );
        }
        let Some(world) = worlds.get_mut(render_key) else {
            return false;
        };
        let changed = world.reconcile_curated(original, profile, snapshot);
        if (changed || self.curated_dirty) && self.preview.is_none() {
            if let Some(writer) = &self.curated_writer {
                writer.save(&self.curated_worlds);
            }
            self.curated_dirty = false;
        }
        changed
    }
}
