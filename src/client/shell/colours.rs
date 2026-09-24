//! Client-owned workspace colour families. No colour math is performed while drawing.
mod allocator;
mod bank;
mod catalogue;
mod math;
mod persistence;
#[cfg(test)]
mod tests;

use super::*;
use allocator::Workspace;
use bank::{Candidate, Rng};
use catalogue::THEMES;
use math::*;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct World {
    rng: Rng,
    workspaces: HashMap<String, Workspace>,
}

impl Default for World {
    fn default() -> Self {
        // RandomState obtains process entropy through the standard library. Each
        // workspace gets its own advancing streams; deleting a tab never rewinds them.
        let seed = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        Self {
            rng: Rng(seed),
            workspaces: HashMap::new(),
        }
    }
}

impl World {
    fn reconcile(&mut self, snapshot: &ClientShellSnapshot) -> bool {
        let mut changed = false;
        let old_len = self.workspaces.len();
        self.workspaces
            .retain(|id, _| snapshot.workspaces.iter().any(|w| &w.workspace_id == id));
        changed |= old_len != self.workspaces.len();
        for w in &snapshot.workspaces {
            if !self.workspaces.contains_key(&w.workspace_id) {
                let parents: Vec<Candidate> =
                    self.workspaces.values().map(|w| w.bank[w.parent]).collect();
                let unused: Vec<usize> = (0..THEMES.len())
                    .filter(|i| !self.workspaces.values().any(|w| w.theme == *i))
                    .collect();
                let pool: Vec<usize> = if unused.is_empty() {
                    (0..THEMES.len()).collect()
                } else {
                    unused
                };
                let mut best = f64::NEG_INFINITY;
                let mut ranked = Vec::new();
                for i in pool {
                    let p = &THEMES[i];
                    let rep = gamut(
                        (p.light.0 + p.light.1) / 2.,
                        (p.chroma.0 + p.chroma.1) / 2.,
                        p.parent,
                    )
                    .lab();
                    let gap = parents.iter().map(|c| de(rep, c.lab)).fold(100., f64::min);
                    let occupancy = self
                        .workspaces
                        .values()
                        .filter(|w| THEMES[w.theme].family == p.family)
                        .count();
                    let score = (gap / 22.).min(1.) - 0.65 * occupancy as f64;
                    best = best.max(score);
                    ranked.push((i, score));
                }
                ranked.retain(|(_, s)| *s >= best - 0.10);
                let theme = ranked[self.rng.index(ranked.len())].0;
                if let Some(workspace) = Workspace::new(theme, self.rng.next(), &parents) {
                    self.workspaces.insert(w.workspace_id.clone(), workspace);
                    changed = true;
                } else {
                    tracing::warn!(
                        theme = THEMES[theme].name,
                        "colour bank had too few safe candidates"
                    );
                    continue;
                }
            }
            if let Some(workspace) = self.workspaces.get_mut(&w.workspace_id) {
                // Compare borrowed IDs before allocating or doing any colour work.
                if workspace.tabs.iter().map(|t| t.id.as_str()).eq(snapshot
                    .tabs
                    .iter()
                    .filter(|t| t.workspace_id == w.workspace_id)
                    .map(|t| t.tab_id.as_str()))
                {
                    continue;
                }
                let ids = snapshot
                    .tabs
                    .iter()
                    .filter(|t| t.workspace_id == w.workspace_id)
                    .map(|t| t.tab_id.clone())
                    .collect::<Vec<_>>();
                workspace.reconcile(&ids);
                changed = true;
            }
        }
        changed
    }
}

#[derive(Clone, Copy)]
struct SidebarStyle {
    background: Rgb,
    selected: Rgb,
    title: Rgb,
    detail: Rgb,
}

pub(super) struct Colours {
    worlds: HashMap<String, World>,
    endpoint_keys: HashMap<ClientEndpointId, String>,
    host: crate::terminal_theme::TerminalTheme,
    surfaces: HashMap<String, HashMap<String, Rgb>>,
    sidebar_light: bool,
    sidebar_theme: Option<(Rgb, Rgb, Rgb)>,
    sidebar_styles: HashMap<Rgb, SidebarStyle>,
    tab_styles: HashMap<Rgb, SidebarStyle>,
    writer: Option<persistence::Writer>,
}

impl Colours {
    pub fn enable_persistence(&mut self, path: Option<&std::path::Path>) {
        if self.writer.is_some() {
            return;
        }
        let (worlds, writer) = persistence::open(path);
        if self.worlds.is_empty() {
            self.worlds = worlds;
        }
        self.writer = writer;
        self.refresh_surfaces();
    }
    pub fn new(path: Option<&std::path::Path>) -> Self {
        let (worlds, writer) = persistence::open(path);
        Self {
            worlds,
            endpoint_keys: HashMap::new(),
            host: Default::default(),
            surfaces: HashMap::new(),
            sidebar_light: false,
            sidebar_theme: None,
            sidebar_styles: HashMap::new(),
            tab_styles: HashMap::new(),
            writer,
        }
    }
    pub fn reconcile(&mut self, endpoint: &ClientEndpointId, snapshot: &ClientShellSnapshot) {
        let key = endpoint.storage_key();
        self.endpoint_keys
            .entry(endpoint.clone())
            .or_insert_with(|| key.clone());
        let world = self.worlds.entry(key.clone()).or_default();
        if world.reconcile(snapshot) {
            self.refresh_surfaces();
            if let Some(writer) = &self.writer {
                writer.save(&self.worlds);
            }
        }
    }
    pub fn update_host(&mut self, event: &crate::raw_input::RawInputEvent) {
        use crate::raw_input::RawInputEvent;
        use crate::terminal_theme::DefaultColorKind;
        let mut next = self.host;
        match event {
            RawInputEvent::HostDefaultColor {
                kind: DefaultColorKind::Foreground,
                color,
            } => next.foreground = Some(*color),
            RawInputEvent::HostDefaultColor {
                kind: DefaultColorKind::Background,
                color,
            } => next.background = Some(*color),
            RawInputEvent::HostPaletteColors { colors } => {
                for (i, c) in colors {
                    next.palette[usize::from(*i)] = Some(*c);
                }
            }
            _ => return,
        }
        if next != self.host {
            self.host = next;
            self.refresh_surfaces();
        }
    }
    fn refresh_surfaces(&mut self) {
        self.refresh_sidebar_styles();
        self.surfaces.clear();
        self.sidebar_light = self
            .host
            .background
            .is_some_and(|bg| Rgb([bg.r, bg.g, bg.b]).oklch()[0] > 0.65);
        let (Some(bg), Some(fg)) = (self.host.background, self.host.foreground) else {
            return;
        };
        let rgb = |c: crate::terminal_theme::RgbColor| Rgb([c.r, c.g, c.b]);
        let bg = rgb(bg);
        let fg = rgb(fg);
        let l = bg.oklch()[0];
        for (endpoint, world) in &self.worlds {
            for (id, w) in &world.workspaces {
                let hue = w.bank[w.parent].header.oklch()[2];
                let mut surface = bg;
                for c in [0.006, 0.004, 0.002] {
                    let proposed = gamut(l, c, hue);
                    let luminance_ratio = (proposed.luminance() + 0.05) / (bg.luminance() + 0.05);
                    if (0.9..=1. / 0.9).contains(&luminance_ratio)
                        && contrast(fg, proposed) >= contrast(fg, bg).min(4.5)
                        && self
                            .host
                            .palette
                            .iter()
                            .flatten()
                            .all(|a| contrast(rgb(*a), proposed) >= 0.9 * contrast(rgb(*a), bg))
                        && w.tabs
                            .iter()
                            .all(|t| contrast(w.bank[t.colour].accent, proposed) >= 3.)
                    {
                        surface = proposed;
                        break;
                    }
                }
                self.surfaces
                    .entry(endpoint.clone())
                    .or_default()
                    .insert(id.clone(), surface);
            }
        }
    }
    fn workspace(&self, endpoint: &ClientEndpointId, id: &str) -> Option<&Workspace> {
        self.worlds
            .get(self.endpoint_keys.get(endpoint)?)?
            .workspaces
            .get(id)
    }
    pub fn surface(
        &self,
        endpoint: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
    ) -> Option<u32> {
        let id = snapshot.focused_workspace_id.as_ref()?;
        self.surfaces
            .get(self.endpoint_keys.get(endpoint)?)?
            .get(id)
            .map(|c| crate::protocol::color_to_u32(c.color()))
    }
    // Theme changes and topology changes refresh these small caches. Drawing only
    // looks up ready-to-use RGB values; no per-frame colour generation is needed.
    pub fn set_sidebar_theme(&mut self, palette: &Palette) {
        let rgb = |c| match c {
            Color::Rgb(r, g, b) => Some(Rgb([r, g, b])),
            _ => None,
        };
        let background = rgb(palette.sidebar_bg)
            .or_else(|| rgb(palette.panel_bg))
            .unwrap_or(bank::SIDEBAR[usize::from(self.sidebar_light)]);
        let active = rgb(palette.active_row_bg).unwrap_or(background);
        let tab_background = rgb(palette.panel_bg).unwrap_or(background);
        if self.sidebar_theme != Some((background, active, tab_background)) {
            self.sidebar_theme = Some((background, active, tab_background));
            self.refresh_sidebar_styles();
        }
    }
    fn refresh_sidebar_styles(&mut self) {
        let Some((background, active, tab_background)) = self.sidebar_theme else {
            return;
        };
        self.sidebar_styles.clear();
        self.tab_styles.clear();
        for world in self.worlds.values() {
            for w in world.workspaces.values() {
                for index in std::iter::once(w.parent).chain(w.tabs.iter().map(|t| t.colour)) {
                    let c = w.bank[index];
                    self.sidebar_styles
                        .entry(c.header)
                        .or_insert_with(|| chrome_style(c, background, active, false));
                    self.tab_styles
                        .entry(c.header)
                        .or_insert_with(|| chrome_style(c, tab_background, active, true));
                }
            }
        }
    }
    pub fn paint_sidebar(&self, buffer: &mut Buffer, area: Rect, palette: &Palette) {
        let background = self
            .sidebar_theme
            .map(|(bg, _, _)| bg)
            .unwrap_or(bank::SIDEBAR[usize::from(self.sidebar_light)])
            .color();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_bg(background);
                    // Restore normal theme typography for controls; family text is
                    // applied to workspace/agent rows below.
                    if cell.fg == palette.mauve {
                        cell.set_fg(palette.overlay0);
                    }
                }
            }
        }
    }
    fn paint_sidebar_row(
        &self,
        buffer: &mut Buffer,
        rect: Rect,
        c: Candidate,
        selected: bool,
        palette: &Palette,
    ) {
        let Some(style) = self.sidebar_styles.get(&c.header) else {
            return;
        };
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_bg(
                        if selected {
                            style.selected
                        } else {
                            style.background
                        }
                        .color(),
                    );
                    if !semantic_cell(cell, palette) {
                        cell.set_fg(
                            if y == rect.y {
                                style.title
                            } else {
                                style.detail
                            }
                            .color(),
                        );
                    }
                    cell.modifier.remove(Modifier::UNDERLINED);
                    if selected && y == rect.y {
                        cell.modifier.insert(Modifier::BOLD);
                    }
                }
            }
        }
    }
    pub fn paint_agents(
        &self,
        buffer: &mut Buffer,
        hits: &ShellHitMap,
        active: (&ClientEndpointId, &ClientShellSnapshot),
        endpoints: &[ClientShellEndpoint],
        palette: &Palette,
    ) {
        let rows = hits
            .agents
            .iter()
            .map(|(rect, pane)| (rect, active.0, pane))
            .chain(
                hits.endpoint_agents
                    .iter()
                    .map(|(rect, endpoint, pane)| (rect, endpoint, pane)),
            );
        for (rect, endpoint, pane) in rows {
            let snapshot = if endpoint == active.0 {
                Some(active.1)
            } else {
                endpoints
                    .iter()
                    .find(|e| &e.endpoint_id == endpoint)
                    .and_then(|e| e.snapshot.as_deref())
            };
            let Some(agent) = snapshot.and_then(|s| s.agents.iter().find(|a| &a.pane_id == pane))
            else {
                continue;
            };
            let Some(w) = self.workspace(endpoint, &agent.workspace_id) else {
                continue;
            };
            let Some(tab) = w.tabs.iter().find(|t| t.id == agent.tab_id) else {
                continue;
            };
            self.paint_sidebar_row(
                buffer,
                *rect,
                w.bank[tab.colour],
                endpoint == active.0 && agent.focused,
                palette,
            );
        }
    }
    pub fn paint_chrome(
        &self,
        buffer: &mut Buffer,
        hits: &ShellHitMap,
        endpoint: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
        palette: &Palette,
        navigation: Option<&WorkspaceNavigationTarget>,
    ) {
        for hit in &hits.workspaces {
            if let Some(w) = self.workspace(&hit.endpoint_id, &hit.workspace_id) {
                let selected = navigation
                    .is_some_and(|target| target.matches(&hit.endpoint_id, &hit.workspace_id))
                    || snapshot
                        .workspaces
                        .iter()
                        .any(|s| s.workspace_id == hit.workspace_id && s.focused)
                        && &hit.endpoint_id == endpoint;
                self.paint_sidebar_row(buffer, hit.rect, w.bank[w.parent], selected, palette);
            }
        }
        let Some(w) = snapshot
            .focused_workspace_id
            .as_deref()
            .and_then(|id| self.workspace(endpoint, id))
        else {
            return;
        };
        for (rect, id) in &hits.tabs {
            if let Some(t) = w.tabs.iter().find(|t| &t.id == id) {
                let selected = snapshot.tabs.iter().any(|s| &s.tab_id == id && s.focused);
                if let Some(style) = self.tab_styles.get(&w.bank[t.colour].header) {
                    paint_tab(buffer, *rect, *style, selected, palette);
                }
            }
        }
    }
}

fn semantic_cell(cell: &ratatui::buffer::Cell, palette: &Palette) -> bool {
    [
        palette.green,
        palette.yellow,
        palette.red,
        palette.blue,
        palette.peach,
    ]
    .contains(&cell.fg)
        && cell.symbol() != " "
}

fn chrome_style(c: Candidate, background: Rgb, active: Rgb, tab: bool) -> SidebarStyle {
    let selected = Rgb(std::array::from_fn(|i| {
        ((u16::from(active.0[i]) * 9 + u16::from(c.header.0[i])) / 10) as u8
    }));
    let hue = c.header.oklch()[2];
    let light = background.luminance() > 0.4;
    let text = |l, chroma| {
        let tint = gamut(l, chroma, hue);
        if contrast(tint, background) >= 4.5 && contrast(tint, selected) >= 4.5 {
            tint
        } else if contrast(bank::LIGHT, background).min(contrast(bank::LIGHT, selected))
            > contrast(bank::INK, background).min(contrast(bank::INK, selected))
        {
            bank::LIGHT
        } else {
            bank::INK
        }
    };
    SidebarStyle {
        background,
        selected,
        title: text(
            if light {
                0.38
            } else if tab {
                0.83
            } else {
                0.79
            },
            if tab { 0.065 } else { 0.045 },
        ),
        detail: text(
            if light { 0.45 } else { 0.69 },
            if tab { 0.045 } else { 0.018 },
        ),
    }
}

fn paint_tab(
    buffer: &mut Buffer,
    rect: Rect,
    style: SidebarStyle,
    selected: bool,
    palette: &Palette,
) {
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_bg(
                    if selected {
                        style.selected
                    } else {
                        style.background
                    }
                    .color(),
                );
                if !semantic_cell(cell, palette) {
                    cell.set_fg(if selected { style.title } else { style.detail }.color());
                }
                // Explicit contrast-checked colours rather than terminal DIM,
                // whose intensity varies across emulators and can become unreadable.
                cell.modifier
                    .remove(Modifier::UNDERLINED | Modifier::BOLD | Modifier::DIM);
                if selected {
                    cell.modifier.insert(Modifier::BOLD);
                }
            }
        }
    }
}

pub(super) fn tint_default(cell: &mut crate::protocol::CellData, tint: Option<u32>) {
    if cell.bg == 0 {
        if let Some(bg) = tint {
            cell.bg = bg;
        }
    }
}
