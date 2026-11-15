//! Near fields: E and H at any point from the currents the engines found, for RF
//! exposure checks.
//!
//! Every engine's currents are turned into short straight filaments whose current
//! varies linearly from one end to the other (wiremom's own shape; MININEC's halves
//! are constant; NEC-2's segment-centre currents are joined up linearly). A filament
//! carries the line charge that its change of current implies and point charges at its
//! ends, which cancel along a wire and leave the right charge at junctions and ends,
//! so E = −jωA − ∇φ and H = ∇×A/μ0 hold for the current as a whole.
//!
//! Perfect ground adds exact images. Real ground adds images weighted by the plane-wave
//! reflection coefficients at the specular angle (the reflection coefficient
//! approximation): good a few tenths of a wavelength above the ground, rougher close
//! to it.

use crate::model::{Antenna, GroundKind};
use num_complex::Complex64 as C;
use rayon::prelude::*;
use std::f64::consts::PI;

type V3 = [f64; 3];

const C0: f64 = 299_792_458.0;
const MU0: f64 = 4e-7 * PI;
const EPS0: f64 = 1.0 / (MU0 * C0 * C0);
const ETA0: f64 = MU0 * C0;

/// A straight piece of current: `ia` at `a` rising or falling linearly to `ib` at `b`,
/// both measured in the direction a → b (peak amps).
#[derive(Clone, Copy, Debug)]
pub struct Filament {
    pub a: V3,
    pub b: V3,
    pub ia: C,
    pub ib: C,
    /// wire radius, which keeps the field finite on the wire itself
    pub r: f64,
}

/// What the near field needs from a calculation.
#[derive(Clone, Debug, Default)]
pub struct Currents {
    pub filaments: Vec<Filament>,
    /// Power the sources deliver (W) at these currents.
    pub p_in: f64,
    /// Plates were left out (their surface currents aren't included).
    pub plates_left_out: bool,
}

/// MININEC: each half segment carries a constant current.
pub fn from_halves(segs: &[crate::mom::Seg], half: &[[C; 2]]) -> Vec<Filament> {
    let mut out = Vec::with_capacity(segs.len() * 2);
    for (s, h) in segs.iter().zip(half) {
        let m = mid(s.a, s.b);
        out.push(Filament { a: s.a, b: m, ia: h[0], ib: h[0], r: s.r });
        out.push(Filament { a: m, b: s.b, ia: h[1], ib: h[1], r: s.r });
    }
    out
}

/// NEC-2 and the like: one current at the middle of each segment, (a, b, I along
/// a → b, radius). Along a wire the current runs linearly from one segment centre to
/// the next; it falls to zero at free ends and stays at the segment's value where
/// three or more segments meet.
pub fn from_centres(segs: &[(V3, V3, C, f64)]) -> Vec<Filament> {
    let n = segs.len();
    let tol = |a: f64, b: f64| 1e-4 * a.max(b).max(1e-6);
    let len = |s: &(V3, V3, C, f64)| dist(s.0, s.1);
    // current at an end of segment i (end 0 = a, 1 = b), along a → b
    let end_current = |i: usize, end: usize| -> C {
        let p = if end == 0 { segs[i].0 } else { segs[i].1 };
        let mut others = vec![];
        for (j, s) in segs.iter().enumerate() {
            if j == i {
                continue;
            }
            let t = tol(len(&segs[i]), len(s));
            if dist(s.0, p) < t {
                others.push((j, 0));
            } else if dist(s.1, p) < t {
                others.push((j, 1));
            }
        }
        match others.as_slice() {
            [] => C::new(0.0, 0.0),
            [(j, e)] => {
                // the neighbour's current, turned to run the same way through the node
                let same_way = (end == 1) == (*e == 0);
                let other = if same_way { segs[*j].2 } else { -segs[*j].2 };
                0.5 * (segs[i].2 + other)
            }
            _ => segs[i].2,
        }
    };
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let (a, b, ic, r) = segs[i];
        let m = mid(a, b);
        out.push(Filament { a, b: m, ia: end_current(i, 0), ib: ic, r });
        out.push(Filament { a: m, b, ia: ic, ib: end_current(i, 1), r });
    }
    out
}

/// Ground as the near field sees it.
#[derive(Clone, Copy, Debug)]
enum NearGround {
    Free,
    Perfect,
    /// complex relative permittivity εr − jσ/(ωε0)
    Real(C),
}

/// E and H (peak phasors, V/m and A/m) at the points `pts`, for `cur` at `freq_mhz`.
pub fn fields(cur: &Currents, ant: &Antenna, freq_mhz: f64, pts: &[V3]) -> Vec<([C; 3], [C; 3])> {
    let omega = 2.0 * PI * freq_mhz * 1e6;
    let ground = match ant.ground.kind {
        GroundKind::Free => NearGround::Free,
        GroundKind::Perfect => NearGround::Perfect,
        GroundKind::Real => NearGround::Real(C::new(ant.ground.eps_r, -ant.ground.sigma_ms * 1e-3 / (omega * EPS0))),
    };
    pts.par_iter().map(|p| field_at(&cur.filaments, omega, ground, *p)).collect()
}

/// Gauss–Legendre points and weights on [0, 1].
const GL4: [(f64, f64); 4] = [
    (0.069_431_844_202_973_71, 0.173_927_422_568_726_93),
    (0.330_009_478_207_571_87, 0.326_072_577_431_273_07),
    (0.669_990_521_792_428_1, 0.326_072_577_431_273_07),
    (0.930_568_155_797_026_3, 0.173_927_422_568_726_93),
];

fn field_at(fils: &[Filament], omega: f64, ground: NearGround, p: V3) -> ([C; 3], [C; 3]) {
    let k = omega / C0;
    let jw = C::new(0.0, omega);
    let mut e = [C::new(0.0, 0.0); 3];
    let mut h = [C::new(0.0, 0.0); 3];
    for f in fils {
        add_filament(f, k, jw, p, [C::new(1.0, 0.0); 3], C::new(1.0, 0.0), &mut e, &mut h);
        let img = || Filament { a: mirror(f.a), b: mirror(f.b), ia: -f.ia, ib: -f.ib, r: f.r };
        match ground {
            NearGround::Free => {}
            NearGround::Perfect => add_filament(&img(), k, jw, p, [C::new(1.0, 0.0); 3], C::new(1.0, 0.0), &mut e, &mut h),
            NearGround::Real(n2) => {
                // reflection coefficients at the angle from the image's middle to the point,
                // scaled so both are +1 for a perfect conductor (whose image is built in)
                let im = mirror(mid(f.a, f.b));
                let d = sub(p, im);
                let rr = norm(d);
                let cos_t = if rr > 1e-12 { (d[2] / rr).clamp(0.0, 1.0) } else { 1.0 };
                let root = (n2 - C::new(1.0 - cos_t * cos_t, 0.0)).sqrt();
                let rv = (n2 * cos_t - root) / (n2 * cos_t + root);
                let rh = -((C::new(cos_t, 0.0) - root) / (C::new(cos_t, 0.0) + root));
                // horizontal currents reflect with rh, vertical ones and the charges with rv
                add_filament(&img(), k, jw, p, [rh, rh, rv], rv, &mut e, &mut h);
            }
        }
    }
    (e, h)
}

/// Adds one filament's E and H at `p`. `wc` weights the current's x, y, z parts and
/// `wq` the charges (1 except for real-ground images).
#[allow(clippy::too_many_arguments)]
fn add_filament(f: &Filament, k: f64, jw: C, p: V3, wc: [C; 3], wq: C, e: &mut [C; 3], h: &mut [C; 3]) {
    let l = dist(f.a, f.b);
    if l <= 0.0 {
        return;
    }
    let u = scale(sub(f.b, f.a), 1.0 / l);
    // closer than a few lengths: split it up so the sum stays accurate
    let d = dist(p, mid(f.a, f.b)).max(f.r);
    let lambda = 2.0 * PI / k;
    let n = ((3.0 * l / d).ceil().max((20.0 * l / lambda).ceil()) as usize).clamp(1, 64);
    let dl = l / n as f64;
    let rho = -(f.ib - f.ia) / (jw * l); // line charge, C/m
    let cq = wq / (4.0 * PI * EPS0);
    let ca = MU0 / (4.0 * PI);
    for piece in 0..n {
        for &(x, w) in &GL4 {
            let t = (piece as f64 + x) / n as f64;
            let s = add(f.a, scale(u, t * l));
            let i = f.ia + (f.ib - f.ia) * t;
            let (g, dg) = green(p, s, f.r, k);
            let wl = w * dl;
            // A from the current, H = ∇G × (I û) / 4π
            for c in 0..3 {
                e[c] -= jw * ca * i * wc[c] * u[c] * g * wl;
            }
            let iu = [i * wc[0] * u[0], i * wc[1] * u[1], i * wc[2] * u[2]];
            let cr = cross_c(dg, iu);
            for c in 0..3 {
                h[c] += cr[c] * (wl / (4.0 * PI));
            }
            // −∇φ from the line charge
            for c in 0..3 {
                e[c] -= cq * rho * dg[c] * wl;
            }
        }
    }
    // point charges at the ends: −I(a)/jω where the current leaves, +I(b)/jω where it arrives
    for (q, at) in [(-f.ia / jw, f.a), (f.ib / jw, f.b)] {
        let (_, dg) = green(p, at, f.r, k);
        for c in 0..3 {
            e[c] -= cq * q * dg[c];
        }
    }
}

/// G = e^{−jkR}/R and its gradient with respect to the field point, with R taken to
/// the wire's surface.
fn green(p: V3, s: V3, r: f64, k: f64) -> (C, [C; 3]) {
    let d = sub(p, s);
    let rr = (dot(d, d) + r * r).sqrt();
    let g = C::from_polar(1.0 / rr, -k * rr);
    let dgdr = -g * C::new(1.0, k * rr) / rr;
    (g, [dgdr * (d[0] / rr), dgdr * (d[1] / rr), dgdr * (d[2] / rr)])
}

/// RMS field strengths |E| (V/m) and |H| (A/m) from peak phasors.
pub fn rms(e: &[C; 3], h: &[C; 3]) -> (f64, f64) {
    let m = |v: &[C; 3]| (v.iter().map(|x| x.norm_sqr()).sum::<f64>() / 2.0).sqrt();
    (m(e), m(h))
}

// ------------------------------------------------------------------ exposure limits

/// Reference levels for the general public (uncontrolled exposure), averaged over time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standard {
    /// ICNIRP 1998 (EU Recommendation 1999/519/EC; Ofcom's EMF licence condition)
    Icnirp1998,
    /// ICNIRP 2020, whole-body averaged
    Icnirp2020,
    /// FCC 47 CFR 1.1310, general population / uncontrolled
    Fcc,
}

impl Standard {
    pub const ALL: [Standard; 3] = [Standard::Icnirp1998, Standard::Icnirp2020, Standard::Fcc];

    pub fn label(&self) -> &'static str {
        match self {
            Standard::Icnirp1998 => "ICNIRP 1998 general public",
            Standard::Icnirp2020 => "ICNIRP 2020 general public",
            Standard::Fcc => "FCC general population",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Standard::Icnirp1998 => "icnirp1998",
            Standard::Icnirp2020 => "icnirp2020",
            Standard::Fcc => "fcc",
        }
    }

    pub fn from_key(k: &str) -> Option<Standard> {
        Standard::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(k))
    }

    /// RMS limits (E V/m, H A/m) at `f` MHz; None outside the range covered.
    pub fn limits(&self, f: f64) -> Option<(f64, f64)> {
        match self {
            Standard::Icnirp1998 => Some(match f {
                f if f < 0.15 => return None,
                f if f < 1.0 => (87.0, 0.73 / f),
                f if f < 10.0 => (87.0 / f.sqrt(), 0.73 / f),
                f if f < 400.0 => (28.0, 0.073),
                f if f < 2000.0 => (1.375 * f.sqrt(), 0.0037 * f.sqrt()),
                f if f <= 300_000.0 => (61.0, 0.16),
                _ => return None,
            }),
            Standard::Icnirp2020 => Some(match f {
                f if f < 0.1 => return None,
                f if f < 30.0 => (300.0 / f.powf(0.7), 2.2 / f),
                f if f < 400.0 => (27.7, 0.073),
                f if f < 2000.0 => (1.375 * f.sqrt(), 0.0037 * f.sqrt()),
                // above 2 GHz only power density is given (10 W/m²): its plane-wave fields
                f if f <= 300_000.0 => ((10.0 * ETA0).sqrt(), (10.0 / ETA0).sqrt()),
                _ => return None,
            }),
            Standard::Fcc => {
                // power density in mW/cm² for the plane-wave equivalents above 300 MHz
                let s = |mw_cm2: f64| -> (f64, f64) {
                    let w_m2 = mw_cm2 * 10.0;
                    ((w_m2 * ETA0).sqrt(), (w_m2 / ETA0).sqrt())
                };
                Some(match f {
                    f if f < 0.3 => return None,
                    f if f < 1.34 => (614.0, 1.63),
                    f if f < 30.0 => (824.0 / f, 2.19 / f),
                    f if f < 300.0 => (27.5, 0.073),
                    f if f < 1500.0 => s(f / 1500.0),
                    f if f <= 100_000.0 => s(1.0),
                    _ => return None,
                })
            }
        }
    }
}

/// Exposure at a point as a fraction of the limit: the larger of (E/E_lim)² and
/// (H/H_lim)², which are the power-density equivalents and so add up over time.
pub fn ratio(e_rms: f64, h_rms: f64, lim: (f64, f64)) -> f64 {
    (e_rms / lim.0).powi(2).max((h_rms / lim.1).powi(2))
}

// ------------------------------------------------------------------ small vector helpers

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn dist(a: V3, b: V3) -> f64 {
    norm(sub(a, b))
}
fn mid(a: V3, b: V3) -> V3 {
    scale(add(a, b), 0.5)
}
fn mirror(a: V3) -> V3 {
    [a[0], a[1], -a[2]]
}
fn cross_c(a: [C; 3], b: [C; 3]) -> [C; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Engine;

    /// Far out, the power density from the near-field sum must match the gain the
    /// engine reports: S = G·P/(4πr²), with E as an RMS value.
    #[test]
    fn far_out_matches_the_gain() {
        for e in [Engine::Wiremom, Engine::Mininec] {
            let mut a = Antenna::example(0);
            a.engine = e;
            let c = crate::mom::calculate(&a).unwrap();
            let r = 300.0;
            // the dipole lies along y: broadside is along x
            let f = fields(&c.near, &a, a.freq_mhz, &[[r, 0.0, 0.0]]);
            let (er, hr) = rms(&f[0].0, &f[0].1);
            let s = er * er / ETA0;
            let g = 10f64.powf(c.result.ga_dbi / 10.0);
            let want = g * c.near.p_in / (4.0 * PI * r * r);
            assert!((s / want - 1.0).abs() < 0.02, "{e:?}: S {s:.4e} vs {want:.4e}");
            // and a plane wave there: E/H = η0
            assert!((er / hr / ETA0 - 1.0).abs() < 0.01, "{e:?}: E/H {}", er / hr);
        }
    }

    #[test]
    fn limits() {
        let close = |a: f64, b: f64| (a / b - 1.0).abs() < 1e-3;
        let (e, h) = Standard::Icnirp1998.limits(14.15).unwrap();
        assert!(close(e, 28.0) && close(h, 0.073));
        let (e, _) = Standard::Icnirp1998.limits(7.1).unwrap();
        assert!(close(e, 87.0 / 7.1f64.sqrt()));
        let (e, h) = Standard::Fcc.limits(14.15).unwrap();
        assert!(close(e, 824.0 / 14.15) && close(h, 2.19 / 14.15));
        // FCC 1 mW/cm² above 1.5 GHz is 10 W/m², about 61.4 V/m
        let (e, _) = Standard::Fcc.limits(2400.0).unwrap();
        assert!((e - 61.4).abs() < 0.1, "{e}");
        let (e, h) = Standard::Icnirp2020.limits(144.0).unwrap();
        assert!(close(e, 27.7) && close(h, 0.073));
        assert_eq!(Standard::Fcc.limits(0.1), None);
    }
}
