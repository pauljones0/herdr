//! Colour calculations use rendered sRGB8 values. Lab is D65, not OKLab.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn color(self) -> ratatui::style::Color {
        let [r, g, b] = self.0;
        ratatui::style::Color::Rgb(r, g, b)
    }
    fn linear(self) -> [f64; 3] {
        self.0.map(|v| {
            let x = f64::from(v) / 255.;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        })
    }
    pub fn luminance(self) -> f64 {
        let [r, g, b] = self.linear();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
    pub fn lab(self) -> [f64; 3] {
        let [r, g, b] = self.linear();
        let f = |t: f64| {
            if t > (6_f64 / 29.).powi(3) {
                t.cbrt()
            } else {
                t / (3. * (6_f64 / 29.).powi(2)) + 4. / 29.
            }
        };
        let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
        let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
        let z = f((0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883);
        [116. * y - 16., 500. * (x - y), 200. * (y - z)]
    }
    pub fn oklch(self) -> [f64; 3] {
        let [r, g, b] = self.linear();
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
        let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            a.hypot(b),
            b.atan2(a).to_degrees().rem_euclid(360.),
        ]
    }
}

pub(super) fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (a.luminance(), b.luminance());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
pub(super) fn hue_gap(a: f64, b: f64) -> f64 {
    ((a - b + 180.).rem_euclid(360.) - 180.).abs()
}

/// Reduce chroma at fixed OKLCH lightness/hue, then round ties to even like the demo.
pub(super) fn gamut(l: f64, c: f64, h: f64) -> Rgb {
    let convert = |c: f64| {
        let a = c * h.to_radians().cos();
        let b = c * h.to_radians().sin();
        let x = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let y = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let z = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        [
            4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z,
            -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z,
            -0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z,
        ]
    };
    let valid = |rgb: [f64; 3]| rgb.iter().all(|v| (0.0..=1.0).contains(v));
    let mut rgb = convert(c);
    if !valid(rgb) {
        let (mut lo, mut hi) = (0., c);
        for _ in 0..25 {
            let mid = (lo + hi) / 2.;
            if valid(convert(mid)) {
                lo = mid
            } else {
                hi = mid
            }
        }
        rgb = convert(lo);
    }
    Rgb(rgb.map(|v| {
        let x = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        };
        (x.clamp(0., 1.) * 255.).round_ties_even() as u8
    }))
}

/// Sharma/Wu/Dalal CIEDE2000, unit parametric factors.
pub(super) fn de(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [l1, a1, b1] = a;
    let [l2, a2, b2] = b;
    let c = (a1.hypot(b1) + a2.hypot(b2)) / 2.;
    let g = 0.5 * (1. - (c.powi(7) / (c.powi(7) + 25_f64.powi(7))).sqrt());
    let (a1, a2) = ((1. + g) * a1, (1. + g) * a2);
    let (c1, c2) = (a1.hypot(b1), a2.hypot(b2));
    let h = |a: f64, b: f64| {
        if a == 0. && b == 0. {
            0.
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.)
        }
    };
    let (h1, h2) = (h(a1, b1), h(a2, b2));
    let dl = l2 - l1;
    let dc = c2 - c1;
    let dh = if c1 * c2 == 0. {
        0.
    } else if (h2 - h1).abs() <= 180. {
        h2 - h1
    } else if h2 <= h1 {
        h2 - h1 + 360.
    } else {
        h2 - h1 - 360.
    };
    let dh = 2. * (c1 * c2).sqrt() * (dh.to_radians() / 2.).sin();
    let lm = (l1 + l2) / 2.;
    let cm = (c1 + c2) / 2.;
    let hm = if c1 * c2 == 0. {
        h1 + h2
    } else if (h1 - h2).abs() <= 180. {
        (h1 + h2) / 2.
    } else if h1 + h2 < 360. {
        (h1 + h2 + 360.) / 2.
    } else {
        (h1 + h2 - 360.) / 2.
    };
    let cos = |x: f64| x.to_radians().cos();
    let t = 1. - 0.17 * cos(hm - 30.) + 0.24 * cos(2. * hm) + 0.32 * cos(3. * hm + 6.)
        - 0.20 * cos(4. * hm - 63.);
    let sl = 1. + 0.015 * (lm - 50.).powi(2) / (20. + (lm - 50.).powi(2)).sqrt();
    let sc = 1. + 0.045 * cm;
    let sh = 1. + 0.015 * cm * t;
    let rt = -2.
        * (cm.powi(7) / (cm.powi(7) + 25_f64.powi(7))).sqrt()
        * (60. * (-((hm - 275.) / 25.).powi(2)).exp())
            .to_radians()
            .sin();
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dh / sh).powi(2) + rt * (dc / sc) * (dh / sh))
        .max(0.)
        .sqrt()
}

pub(super) fn tonal(labs: impl Iterator<Item = [f64; 3]> + Clone) -> f64 {
    let points = labs.map(|[l, a, b]| {
        let c = a.hypot(b);
        let sl = 1. + 0.015 * (l - 50.).powi(2) / (20. + (l - 50.).powi(2)).sqrt();
        (c, l, (1. + 0.045 * c + 2. * sl).powi(-2))
    });
    let (mut w, mut x, mut y) = (0., 0., 0.);
    for (c, l, v) in points.clone() {
        w += v;
        x += c * v;
        y += l * v;
    }
    if w == 0. {
        return 0.;
    }
    x /= w;
    y /= w;
    let (mut xx, mut yy, mut xy) = (0., 0., 0.);
    for (c, l, v) in points.clone() {
        xx += v * (c - x).powi(2);
        yy += v * (l - y).powi(2);
        xy += v * (c - x) * (l - y);
    }
    let theta = 0.5 * (2. * xy).atan2(xx - yy);
    let (nx, ny) = (-theta.sin(), theta.cos());
    let (mut total, mut n) = (0., 0.);
    for (c, l, _) in points {
        total += ((c - x) * nx + (l - y) * ny).abs();
        n += 1.;
    }
    total / n
}
