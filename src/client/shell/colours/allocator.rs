use super::{bank::*, catalogue::THEMES, math::*};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

// Newly introduced conflicts, total conflicts, displacement, stable candidate index.
type RepairRank = (usize, usize, u64, usize);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Tab {
    pub id: String,
    pub colour: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Workspace {
    pub theme: usize,
    pub seed: u64,
    #[serde(default = "legacy_raw")]
    pub raw: usize,
    pub parent: usize,
    pub allocation: Rng,
    pub movement: Rng,
    pub history: VecDeque<usize>,
    pub tabs: Vec<Tab>,
    #[serde(skip)]
    pub bank: Vec<Candidate>,
    #[serde(skip)]
    pub remaining_conflicts: usize,
}

fn legacy_raw() -> usize {
    4096
}

impl Workspace {
    pub fn new(theme: usize, seed: u64, parents: &[Candidate]) -> Option<Self> {
        let bank = generate(theme, seed, 1024);
        if bank.len() < 64 {
            return None;
        }
        let mut rng = Rng(seed ^ 0x117);
        let mut choices = Vec::new();
        let mut best = f64::NEG_INFINITY;
        for (i, c) in bank.iter().enumerate() {
            if hue_gap(c.header.oklch()[2], THEMES[theme].parent) > 22. {
                continue;
            }
            let score = parents
                .iter()
                .map(|p| de(c.lab, p.lab))
                .fold(100., f64::min);
            if score > best {
                best = score;
            }
            choices.push((i, score));
        }
        choices.retain(|(_, score)| *score >= best - 0.25);
        let parent = if choices.is_empty() {
            0
        } else {
            choices[rng.index(choices.len())].0
        };
        Some(Self {
            theme,
            seed,
            raw: 1024,
            parent,
            allocation: Rng(seed ^ 0x118),
            movement: Rng(seed ^ 0x119),
            history: VecDeque::new(),
            tabs: Vec::new(),
            bank,
            remaining_conflicts: 0,
        })
    }
    pub fn restore(&mut self) -> bool {
        if self.theme >= THEMES.len() || ![1024, 4096].contains(&self.raw) {
            return false;
        }
        self.bank = generate(self.theme, self.seed, self.raw);
        self.parent < self.bank.len()
            && self.tabs.iter().all(|t| t.colour < self.bank.len())
            && self.history.iter().all(|i| *i < self.bank.len())
            && self.history.len() <= 16
    }
    fn expand(&mut self) {
        self.raw = 4096;
        self.bank = generate(self.theme, self.seed, self.raw);
    }
    fn neighbours(&self, pos: usize) -> impl Iterator<Item = usize> {
        [
            pos.checked_sub(1),
            (pos + 1 < self.tabs.len()).then_some(pos + 1),
        ]
        .into_iter()
        .flatten()
    }
    fn quality(&self, pos: usize, c: usize) -> (usize, f64) {
        let mut bad = 0;
        let mut gap = 100_f64;
        for n in self.neighbours(pos) {
            if let Some(other) = self.bank.get(self.tabs[n].colour) {
                let d = self.bank[c].gap(*other);
                bad += usize::from(d < 1.);
                gap = gap.min(d);
            }
        }
        (bad, gap)
    }
    fn remember(&mut self, c: usize) {
        self.history.push_back(c);
        if self.history.len() > 16 {
            self.history.pop_front();
        }
    }
    fn choose(&mut self, pos: usize, moved: bool) {
        let mut candidates = Vec::with_capacity(self.bank.len());
        let mut min_bad = usize::MAX;
        let mut max_gap = 0_f64;
        for c in 0..self.bank.len() {
            let (bad, gap) = self.quality(pos, c);
            if bad < min_bad {
                min_bad = bad;
                max_gap = gap;
                candidates.clear();
            }
            if bad == min_bad {
                max_gap = max_gap.max(gap);
                candidates.push((c, gap));
            }
        }
        if min_bad > 0 && self.raw < 4096 {
            self.expand();
            return self.choose(pos, moved);
        }
        if min_bad > 0 {
            candidates.retain(|(_, gap)| *gap >= max_gap - 0.02);
        }
        let remembered: HashSet<usize> = self
            .tabs
            .iter()
            .map(|t| t.colour)
            .chain(self.history.iter().copied())
            .collect();
        if candidates.iter().any(|(c, _)| !remembered.contains(c)) {
            candidates.retain(|(c, _)| !remembered.contains(c));
        }
        // Score a uniform sample of the feasible pool. The full bank still decides
        // feasibility, so sampling cannot manufacture a neighbour conflict.
        if candidates.len() > 256 {
            let rng = if moved {
                &mut self.movement
            } else {
                &mut self.allocation
            };
            for i in 0..256 {
                let j = i + rng.index(candidates.len() - i);
                candidates.swap(i, j);
            }
            candidates.truncate(256);
        }
        let others: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, t)| *i != pos && t.colour < self.bank.len())
            .map(|(_, t)| self.bank[t.colour].lab)
            .collect();
        let mut best = f64::NEG_INFINITY;
        let mut scores = Vec::with_capacity(candidates.len());
        for (c, gap) in candidates {
            let lab = self.bank[c].lab;
            let nearest = others.iter().map(|o| de(lab, *o)).fold(100., f64::min);
            let mut score = (gap / (18. / 7.)).min(1.);
            if !others.is_empty() {
                score += 0.3 * (nearest / 12.).min(1.);
                let residual = tonal(others.iter().copied().chain(std::iter::once(lab)));
                score = 0.65 * score + 0.5 * (-(residual / 3.).powi(2)).exp();
            }
            if !self.history.is_empty() {
                let recent = self
                    .history
                    .iter()
                    .map(|i| de(lab, self.bank[*i].lab))
                    .fold(100., f64::min);
                score -= 0.22 * (-recent / 5.).exp();
            }
            best = best.max(score);
            scores.push((c, score));
        }
        scores.retain(|(_, score)| *score >= best - 0.03);
        if !scores.is_empty() {
            let rng = if moved {
                &mut self.movement
            } else {
                &mut self.allocation
            };
            let c = scores[rng.index(scores.len())].0;
            self.tabs[pos].colour = c;
            self.remember(c);
        }
    }
    pub fn conflicts(&self) -> Vec<(usize, usize)> {
        self.tabs
            .windows(2)
            .enumerate()
            .filter_map(|(i, t)| {
                (self.bank[t[0].colour].gap(self.bank[t[1].colour]) < 1.).then_some((i, i + 1))
            })
            .collect()
    }
    /// Snapshot reconciliation is idempotent. Focus, labels and agent state never reach here.
    pub fn reconcile(&mut self, ids: &[String]) {
        if self.tabs.iter().map(|t| &t.id).eq(ids.iter()) {
            return;
        }
        let before: HashMap<_, _> = self.tabs.iter().map(|t| (t.id.clone(), t.colour)).collect();
        let old_edges: HashSet<_> = self
            .tabs
            .windows(2)
            .map(|t| (t[0].id.clone(), t[1].id.clone()))
            .collect();
        // A single move can shift many indices. Find the one omitted ID whose removal
        // leaves the order unchanged; it gets the direct-move recolour allowance.
        let moved = if ids.len() == self.tabs.len() && ids.iter().all(|id| before.contains_key(id))
        {
            ids.iter()
                .find(|id| {
                    self.tabs
                        .iter()
                        .filter(|t| &t.id != *id)
                        .map(|t| &t.id)
                        .eq(ids.iter().filter(|other| *other != *id))
                })
                .cloned()
        } else {
            None
        };
        self.tabs = ids
            .iter()
            .map(|id| Tab {
                id: id.clone(),
                colour: before.get(id).copied().unwrap_or(usize::MAX),
            })
            .collect();
        for pos in 0..self.tabs.len() {
            if self.tabs[pos].colour == usize::MAX {
                self.choose(pos, false);
            }
        }
        if let Some(id) = &moved {
            if let Some(pos) = self.tabs.iter().position(|t| &t.id == id) {
                if self.quality(pos, self.tabs[pos].colour).0 > 0 {
                    self.choose(pos, true);
                }
            }
        }
        self.repair(&before, &old_edges, moved.as_deref(), 2);
        self.remaining_conflicts = self.conflicts().len();
    }
    pub(super) fn repair(
        &mut self,
        before: &HashMap<String, usize>,
        old_edges: &HashSet<(String, String)>,
        primary: Option<&str>,
        budget: usize,
    ) {
        let mut changed = HashSet::new();
        let mut attempts = 0;
        let mut cache = DistanceCache::default();
        loop {
            let conflicts = self.conflicts();
            if conflicts.is_empty() || attempts >= 96 {
                break;
            }
            let mut endpoints: Vec<_> = conflicts.iter().flat_map(|(a, b)| [*a, *b]).collect();
            endpoints.sort_unstable();
            endpoints.dedup();
            endpoints.sort_by_key(|i| {
                let new = conflicts
                    .iter()
                    .filter(|(a, b)| {
                        (*a == *i || *b == *i)
                            && !old_edges
                                .contains(&(self.tabs[*a].id.clone(), self.tabs[*b].id.clone()))
                    })
                    .count();
                (std::cmp::Reverse(new), *i)
            });
            let permitted = |i: usize, changed: &HashSet<usize>| {
                primary == Some(self.tabs[i].id.as_str())
                    || changed.contains(&i)
                    || changed.len() < budget
            };
            let original = |i: usize| {
                before
                    .get(&self.tabs[i].id)
                    .copied()
                    .unwrap_or(self.tabs[i].colour)
            };
            let mut best: Option<(RepairRank, usize, usize)> = None;
            for i in endpoints.iter().copied() {
                if !permitted(i, &changed) || attempts >= 96 {
                    continue;
                }
                attempts += 1;
                let current = self.quality(i, self.tabs[i].colour).0;
                for c in 0..self.bank.len() {
                    if !cache.within(&self.bank, c, original(i)) {
                        continue;
                    }
                    let bad = self
                        .neighbours(i)
                        .filter(|n| cache.gap(&self.bank, c, self.tabs[*n].colour) < 1.)
                        .count();
                    if bad >= current {
                        continue;
                    }
                    let new_bad = self
                        .neighbours(i)
                        .filter(|n| {
                            !old_edges.contains(&(
                                self.tabs[i.min(*n)].id.clone(),
                                self.tabs[i.max(*n)].id.clone(),
                            )) && cache.gap(&self.bank, c, self.tabs[*n].colour) < 1.
                        })
                        .count();
                    let rank = (
                        new_bad,
                        conflicts.len() - current + bad,
                        (cache.get(&self.bank, c, original(i))[0] * 1e6) as u64,
                        c,
                    );
                    if best.as_ref().is_none_or(|(r, _, _)| rank < *r) {
                        best = Some((rank, i, c));
                    }
                }
            }
            if let Some((_, i, c)) = best {
                if primary != Some(self.tabs[i].id.as_str()) {
                    changed.insert(i);
                }
                self.tabs[i].colour = c;
                self.remember(c);
                continue;
            }
            // Coordinated pair escape: widening shortlist, bounded work, no random draws.
            let mut pair = None;
            'pairs: for width in [10, 32, 128] {
                for (a, b) in conflicts.iter().copied() {
                    let needed = [a, b]
                        .into_iter()
                        .filter(|i| {
                            primary != Some(self.tabs[*i].id.as_str()) && !changed.contains(i)
                        })
                        .count();
                    if changed.len() + needed > budget || attempts >= 96 {
                        continue;
                    }
                    attempts += 1;
                    let mut shortlist = |i: usize, partner: usize| {
                        let mut list = Vec::new();
                        for c in 0..self.bank.len() {
                            if cache.within(&self.bank, c, original(i))
                                && self
                                    .neighbours(i)
                                    .filter(|n| *n != partner)
                                    .all(|n| cache.gap(&self.bank, c, self.tabs[n].colour) >= 1.)
                            {
                                list.push((c, cache.get(&self.bank, c, original(i))[0]));
                            }
                        }
                        list.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                        list.truncate(width);
                        list
                    };
                    let left = shortlist(a, b);
                    let right = shortlist(b, a);
                    let mut distance = f64::INFINITY;
                    for (ca, da) in &left {
                        for (cb, db) in &right {
                            if da + db < distance && self.bank[*ca].gap(self.bank[*cb]) >= 1. {
                                distance = da + db;
                                pair = Some((a, b, *ca, *cb));
                            }
                        }
                    }
                    if pair.is_some() {
                        break 'pairs;
                    }
                }
            }
            if let Some((a, b, ca, cb)) = pair {
                for (i, c) in [(a, ca), (b, cb)] {
                    if primary != Some(self.tabs[i].id.as_str()) {
                        changed.insert(i);
                    }
                    self.tabs[i].colour = c;
                    self.remember(c);
                }
            } else if self.raw < 4096 && changed.len() < budget {
                self.expand();
                cache = DistanceCache::default();
            } else {
                break;
            }
        }
    }
}

/// Only columns actually touched by a repair are cached, never the quadratic bank.
#[derive(Default)]
struct DistanceCache(HashMap<usize, Vec<[f64; 2]>>);
impl DistanceCache {
    fn get(&mut self, bank: &[Candidate], c: usize, against: usize) -> [f64; 2] {
        self.0.entry(against).or_insert_with(|| {
            bank.iter()
                .map(|candidate| {
                    [
                        de(candidate.lab, bank[against].lab),
                        de(candidate.accent_lab, bank[against].accent_lab),
                    ]
                })
                .collect()
        })[c]
    }
    fn gap(&mut self, bank: &[Candidate], c: usize, against: usize) -> f64 {
        let [header, accent] = self.get(bank, c, against);
        (header / 7.).min(accent / 10.)
    }
    fn within(&mut self, bank: &[Candidate], c: usize, against: usize) -> bool {
        let [header, accent] = self.get(bank, c, against);
        header <= 18. && accent <= 25.
    }
}
