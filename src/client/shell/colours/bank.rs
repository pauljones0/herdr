use super::{catalogue::THEMES, math::*};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(super) const LIGHT: Rgb = Rgb([214, 217, 224]);
pub(super) const INK: Rgb = Rgb([16, 20, 27]);

/// SplitMix64 is a small reproducible stream, not a security primitive.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn index(&mut self, n: usize) -> usize {
        ((self.next() as u128 * n as u128) >> 64) as usize
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub header: Rgb,
    pub accent: Rgb,
    pub lab: [f64; 3],
    pub accent_lab: [f64; 3],
}
impl Candidate {
    pub fn gap(self, other: Self) -> f64 {
        (de(self.lab, other.lab) / 7.).min(de(self.accent_lab, other.accent_lab) / 10.)
    }
    #[cfg(test)]
    pub fn within(self, other: Self) -> bool {
        de(self.lab, other.lab) <= 18. && de(self.accent_lab, other.accent_lab) <= 25.
    }
}

pub(super) fn generate(theme: usize, seed: u64, raw: usize) -> Vec<Candidate> {
    let p = &THEMES[theme];
    let mut rng = Rng(seed);
    let rotation = rng.range(-4., 4.);
    let shift = rng.range(-0.008, 0.008);
    let mut result = Vec::with_capacity(raw);
    let mut seen = HashSet::with_capacity(raw);
    for _ in 0..raw {
        let h = p.centres[rng.index(p.centres.len())] + rotation + rng.range(-p.radius, p.radius);
        let u = rng.unit();
        let v = rng.unit();
        let header = gamut(
            p.light.0 + (p.light.1 - p.light.0) * u + shift,
            p.chroma.0 + (p.chroma.1 - p.chroma.0) * v,
            h,
        );
        let accent = gamut(
            rng.range(0.61, 0.81),
            p.accent.0 + (p.accent.1 - p.accent.0) * v,
            h,
        );
        let [l, c, h] = header.oklch();
        let ah = accent.oklch()[2];
        if l < p.light.0 + shift
            || l > p.light.1 + shift
            || c < p.chroma.0
            || c > p.chroma.1
            || !p
                .centres
                .iter()
                .any(|centre| hue_gap(h, *centre + rotation) <= p.radius)
            || !p
                .centres
                .iter()
                .any(|centre| hue_gap(ah, *centre + rotation) <= p.radius)
        {
            continue;
        }
        let ink = if contrast(header, INK) > contrast(header, LIGHT) {
            INK
        } else {
            LIGHT
        };
        if contrast(header, ink) < 4.5 || !seen.insert(header) {
            continue;
        }
        result.push(Candidate {
            header,
            accent,
            lab: header.lab(),
            accent_lab: accent.lab(),
        });
    }
    result
}
