//! The RF exposure window's 3D view: the field on a grid of points around the antenna,
//! and the surfaces where it reaches a share of the limit (100 %, 30 %, 10 %), drawn
//! round the antenna and the ground and turned with the mouse like the 3D pattern.

use crate::model::{Antenna, GroundKind};
use crate::nearfield::{self, Currents, Standard};
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, Vec2};
use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};

type V3 = [f64; 3];

/// Points in the fine grid, and in the first, coarse one that finds how far to reach.
const GRID_POINTS: f64 = 45_000.0;
const COARSE_POINTS: f64 = 8_000.0;

/// Fields on a grid of points, at 1 W into the antenna.
pub struct Volume {
    pub freq_mhz: f64,
    /// corner of the grid and its spacing (m)
    pub lo: V3,
    pub step: f64,
    pub n: [usize; 3],
    /// RMS E (V/m) and H (A/m) at 1 W, x fastest, then y, then z
    pub eh: Vec<(f64, f64)>,
    /// the wires (m)
    pub wires: Vec<(V3, V3)>,
    pub ground: bool,
    /// how far the grid reaches past the antenna (m)
    pub margin: f64,
}

/// How far past the antenna the first grid reaches for `level` (share of the limit) at
/// `avg_w`: the far-field distance where the power density from the antenna's peak gain
/// falls to it, with room to spare, between 1 m and 200 m.
pub fn auto_margin(gain_dbi: f64, avg_w: f64, lim: (f64, f64), level: f64) -> f64 {
    let g = 10f64.powf(gain_dbi / 10.0).max(1.0);
    // the limit's power density: the smaller of the E and H plane-wave equivalents
    let eta = 376.730_313_668;
    let s_lim = (lim.0 * lim.0 / eta).min(lim.1 * lim.1 * eta) * level;
    let r = (g * avg_w.max(1e-3) / (4.0 * std::f64::consts::PI * s_lim)).sqrt();
    (1.5 * r).clamp(1.0, 200.0)
}

/// Fields around the antenna at 1 W into it, on a grid fitted to where the share of the
/// limit at `avg_w` reaches `level`: a coarse grid reaching `margin` past the antenna
/// finds that zone, then a fine one covers it (down to the ground when there is one).
pub fn compute(cur: &Currents, ant: &Antenna, freq_mhz: f64, margin: f64, avg_w: f64, lim: (f64, f64), level: f64) -> Result<Volume, String> {
    if cur.filaments.is_empty() || cur.p_in <= 0.0 {
        return Err("This calculation has no currents to work from: press Calculate first.".into());
    }
    let ground = ant.ground.kind != GroundKind::Free;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for f in &cur.filaments {
        for p in [f.a, f.b] {
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
    }
    let (wlo, whi) = (lo, hi);
    let boxed = |m: f64| -> (V3, V3) {
        let mut a = wlo;
        let mut b = whi;
        for c in 0..3 {
            a[c] -= m;
            b[c] += m;
        }
        if ground {
            a[2] = 0.0;
            b[2] = b[2].max(m);
        }
        (a, b)
    };
    // first pass: where does the outermost surface reach?
    let (clo, chi) = boxed(margin);
    let coarse = grid(cur, ant, freq_mhz, clo, chi, COARSE_POINTS, ground, margin);
    let thr = level.log10();
    let lr = coarse.log_ratio(avg_w, lim);
    let (mut zlo, mut zhi) = (wlo, whi);
    for k in 0..coarse.n[2] {
        for j in 0..coarse.n[1] {
            for i in 0..coarse.n[0] {
                if lr[coarse.idx(i, j, k)] >= thr {
                    let p = coarse.at(i, j, k);
                    for c in 0..3 {
                        zlo[c] = zlo[c].min(p[c]);
                        zhi[c] = zhi[c].max(p[c]);
                    }
                }
            }
        }
    }
    // second pass: that zone and two coarse steps round it, at least a metre past the wires
    let pad = 2.0 * coarse.step;
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for c in 0..3 {
        lo[c] = (zlo[c] - pad).min(wlo[c] - 1.0).max(clo[c]);
        hi[c] = (zhi[c] + pad).max(whi[c] + 1.0).min(chi[c]);
    }
    if ground {
        lo[2] = 0.0;
    }
    let reach = (0..3).map(|c| (wlo[c] - lo[c]).max(hi[c] - whi[c])).fold(0.0, f64::max);
    Ok(grid(cur, ant, freq_mhz, lo, hi, GRID_POINTS, ground, reach))
}

/// Fields on a grid of about `points` points filling the box `lo`–`hi`, at 1 W.
#[allow(clippy::too_many_arguments)]
fn grid(cur: &Currents, ant: &Antenna, freq_mhz: f64, lo: V3, hi: V3, points: f64, ground: bool, margin: f64) -> Volume {
    let vol = (hi[0] - lo[0]) * (hi[1] - lo[1]) * (hi[2] - lo[2]);
    let step = (vol / points).cbrt();
    let n = [0, 1, 2].map(|c| ((hi[c] - lo[c]) / step).ceil() as usize + 1);
    let mut pts = Vec::with_capacity(n[0] * n[1] * n[2]);
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let z = lo[2] + k as f64 * step;
                pts.push([lo[0] + i as f64 * step, lo[1] + j as f64 * step, if ground { z.max(0.01) } else { z }]);
            }
        }
    }
    let f = nearfield::fields(cur, ant, freq_mhz, &pts);
    let s = (1.0 / cur.p_in).sqrt();
    let eh = f.iter().map(|(e, h)| {
        let (er, hr) = nearfield::rms(e, h);
        (er * s, hr * s)
    }).collect();
    let wires = cur.filaments.iter().map(|f| (f.a, f.b)).collect();
    Volume { freq_mhz, lo, step, n, eh, wires, ground, margin }
}

impl Volume {
    fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.n[1] + j) * self.n[0] + i
    }

    fn at(&self, i: usize, j: usize, k: usize) -> V3 {
        [self.lo[0] + i as f64 * self.step, self.lo[1] + j as f64 * self.step, self.lo[2] + k as f64 * self.step]
    }

    /// log10 of each point's share of the limit at `avg_w`.
    fn log_ratio(&self, avg_w: f64, lim: (f64, f64)) -> Vec<f64> {
        let s = avg_w.max(0.0).sqrt();
        self.eh.iter().map(|&(e, h)| nearfield::ratio(e * s, h * s, lim).max(1e-12).log10()).collect()
    }
}

/// What the volume says at a power, against a standard.
pub struct Zone {
    /// farthest a point at or over the limit lies from the wires (m)
    pub reach: Option<f64>,
    /// lowest and highest z of the points at or over the limit
    pub z_range: Option<(f64, f64)>,
    /// the limit is reached at the edge of the grid, so the zone may go further
    pub clipped: bool,
}

/// Whether the surface at `level` runs into the side of the grid (other than the ground).
pub fn clipped(vol: &Volume, avg_w: f64, lim: (f64, f64), level: f64) -> bool {
    let lr = vol.log_ratio(avg_w, lim);
    let thr = level.log10();
    let [nx, ny, nz] = vol.n;
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let edge = i == 0 || j == 0 || i + 1 == nx || j + 1 == ny || k + 1 == nz || (k == 0 && !vol.ground);
                if edge && lr[vol.idx(i, j, k)] >= thr {
                    return true;
                }
            }
        }
    }
    false
}

pub fn zone(vol: &Volume, avg_w: f64, std: Standard) -> Option<Zone> {
    let lim = std.limits(vol.freq_mhz)?;
    let lr = vol.log_ratio(avg_w, lim);
    let mut reach: Option<f64> = None;
    let mut z_range: Option<(f64, f64)> = None;
    let mut clipped = false;
    for k in 0..vol.n[2] {
        for j in 0..vol.n[1] {
            for i in 0..vol.n[0] {
                if lr[vol.idx(i, j, k)] < 0.0 {
                    continue;
                }
                let p = vol.at(i, j, k);
                let d = nearest(&vol.wires, p);
                reach = Some(reach.unwrap_or(0.0).max(d));
                z_range = Some(z_range.map_or((p[2], p[2]), |(a, b)| (a.min(p[2]), b.max(p[2]))));
                let edge = i == 0 || j == 0 || i + 1 == vol.n[0] || j + 1 == vol.n[1] || k + 1 == vol.n[2] || (k == 0 && !vol.ground);
                clipped |= edge;
            }
        }
    }
    Some(Zone { reach, z_range, clipped })
}

fn nearest(wires: &[(V3, V3)], p: V3) -> f64 {
    wires.iter().map(|&(a, b)| {
        let ab = sub(b, a);
        let ap = sub(p, a);
        let l2 = dot(ab, ab);
        let t = if l2 > 0.0 { (dot(ap, ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = sub(ap, scale(ab, t));
        dot(d, d).sqrt()
    }).fold(f64::INFINITY, f64::min)
}

/// The surface where the share of the limit is `level` (1 = 100 %), as triangles,
/// by marching tetrahedra over the grid (each cube split into six), interpolating the
/// log of the share along the edges.
pub fn surface(vol: &Volume, avg_w: f64, lim: (f64, f64), level: f64) -> Vec<[V3; 3]> {
    let lr = vol.log_ratio(avg_w, lim);
    let l0 = level.log10();
    let [nx, ny, nz] = vol.n;
    let mut tris = vec![];
    if nx < 2 || ny < 2 || nz < 2 {
        return tris;
    }
    // cube corners (bit 0 = x, 1 = y, 2 = z) and the six tetrahedra round the 0–7 diagonal
    const TETS: [[usize; 4]; 6] = [[0, 1, 3, 7], [0, 3, 2, 7], [0, 2, 6, 7], [0, 6, 4, 7], [0, 4, 5, 7], [0, 5, 1, 7]];
    for k in 0..nz - 1 {
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let corner = |c: usize| -> (V3, f64) {
                    let (a, b, cc) = (i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                    (vol.at(a, b, cc), lr[vol.idx(a, b, cc)] - l0)
                };
                let cs: [(V3, f64); 8] = std::array::from_fn(corner);
                // skip cubes the surface doesn't cross
                let inside = cs.iter().filter(|c| c.1 >= 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                for t in TETS {
                    tet(&[cs[t[0]], cs[t[1]], cs[t[2]], cs[t[3]]], &mut tris);
                }
            }
        }
    }
    tris
}

/// The surface's piece inside one tetrahedron (values ≥ 0 are inside).
fn tet(v: &[(V3, f64); 4], out: &mut Vec<[V3; 3]>) {
    let cut = |a: (V3, f64), b: (V3, f64)| -> V3 {
        let t = (a.1 / (a.1 - b.1)).clamp(0.0, 1.0);
        add(a.0, scale(sub(b.0, a.0), t))
    };
    let ins: Vec<usize> = (0..4).filter(|&i| v[i].1 >= 0.0).collect();
    let outs: Vec<usize> = (0..4).filter(|&i| v[i].1 < 0.0).collect();
    match ins.len() {
        1 => {
            let a = ins[0];
            out.push([cut(v[a], v[outs[0]]), cut(v[a], v[outs[1]]), cut(v[a], v[outs[2]])]);
        }
        3 => {
            let a = outs[0];
            out.push([cut(v[ins[0]], v[a]), cut(v[ins[1]], v[a]), cut(v[ins[2]], v[a])]);
        }
        2 => {
            let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
            let p = [cut(v[a], v[c]), cut(v[a], v[d]), cut(v[b], v[d]), cut(v[b], v[c])];
            out.push([p[0], p[1], p[2]]);
            out.push([p[0], p[2], p[3]]);
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ the view

/// The view's angle, zoom and pan.
pub struct View3 {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for View3 {
    fn default() -> Self {
        View3 { yaw: -0.6, pitch: 0.35, zoom: 1.0, pan: Vec2::ZERO }
    }
}

/// A surface to draw: its triangles and colour (with alpha).
pub struct Shell<'a> {
    pub tris: &'a [[V3; 3]],
    pub colour: [u8; 4],
}

/// Draws the shells round the antenna and the ground; drag to turn, right-drag to move,
/// wheel to zoom, double-click to reset.
pub fn draw(ui: &mut egui::Ui, size: Vec2, vol: &Volume, shells: &[Shell], v: &mut View3) {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, crate::theme::view_bg());
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, crate::plots::grid()), egui::StrokeKind::Inside);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        v.yaw += d.x * 0.01;
        v.pitch = (v.pitch + d.y * 0.01).clamp(-1.55, 1.55);
    }
    if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
        v.pan += resp.drag_delta();
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            v.zoom = (v.zoom * (1.0 + s * 0.002)).clamp(0.2, 30.0);
        }
    }
    if resp.double_clicked() {
        *v = View3::default();
    }
    // fit the view to the wires and the surfaces shown (and the ground under them)
    let hi = [0, 1, 2].map(|c| vol.lo[c] + (vol.n[c] - 1) as f64 * vol.step);
    let mut wlo = [f64::INFINITY; 3];
    let mut whi = [f64::NEG_INFINITY; 3];
    let pts = vol.wires.iter().flat_map(|(a, b)| [*a, *b]).chain(shells.iter().flat_map(|s| s.tris.iter().flat_map(|t| t.iter().copied())));
    for p in pts {
        for c in 0..3 {
            wlo[c] = wlo[c].min(p[c]);
            whi[c] = whi[c].max(p[c]);
        }
    }
    if vol.ground {
        wlo[2] = 0.0;
    }
    let centre = [0, 1, 2].map(|c| (wlo[c] + whi[c]) / 2.0);
    let half = (0..3).map(|c| (whi[c] - wlo[c]) / 2.0).fold(0.0, f64::max).max(0.5);
    let scale = (size.x.min(size.y) * 0.42 * v.zoom) as f64 / half;
    let (cy, sy, cp, sp) = (v.yaw.cos() as f64, v.yaw.sin() as f64, v.pitch.cos() as f64, v.pitch.sin() as f64);
    let origin = rect.center() + v.pan;
    // camera space: x right, y up, z away from the viewer
    let cam = |p: V3| -> V3 {
        let q = sub(p, centre);
        let x1 = q[0] * cy - q[1] * sy;
        let y1 = q[0] * sy + q[1] * cy;
        [x1, q[2] * cp - y1 * sp, q[2] * sp + y1 * cp]
    };
    let to_screen = |c: V3| origin + Vec2::new((c[0] * scale) as f32, (-c[1] * scale) as f32);
    let painter = painter.with_clip_rect(rect);
    // the ground: a square of the grid's size with a line every metre (or every 5 or 10)
    if vol.ground {
        // the ground under the fitted area, a little wider, within the grid
        let m = 0.15 * half;
        let (x0, x1, y0, y1) = ((wlo[0] - m).max(vol.lo[0]), (whi[0] + m).min(hi[0]), (wlo[1] - m).max(vol.lo[1]), (whi[1] + m).min(hi[1]));
        let quad = [[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]].map(|p| to_screen(cam(p)));
        painter.add(egui::Shape::convex_polygon(quad.to_vec(), Color32::from_rgba_unmultiplied(205, 190, 150, 90), Stroke::new(1.0, Color32::from_rgb(170, 140, 90))));
        let span = (x1 - x0).max(y1 - y0);
        let g = if span < 15.0 { 1.0 } else if span < 60.0 { 5.0 } else { 10.0 };
        let line = Stroke::new(0.5, Color32::from_rgba_unmultiplied(150, 120, 70, 90));
        let mut x = (x0 / g).ceil() * g;
        while x <= x1 {
            painter.line_segment([to_screen(cam([x, y0, 0.0])), to_screen(cam([x, y1, 0.0]))], line);
            x += g;
        }
        let mut y = (y0 / g).ceil() * g;
        while y <= y1 {
            painter.line_segment([to_screen(cam([x0, y, 0.0])), to_screen(cam([x1, y, 0.0]))], line);
            y += g;
        }
        painter.text(rect.left_bottom() + Vec2::new(8.0, -8.0), Align2::LEFT_BOTTOM, format!("ground grid: {g:.0} m squares"), FontId::proportional(11.0), Color32::from_rgb(120, 95, 55));
    }
    // axes from the antenna's centre
    for (i, (col, name)) in [(Color32::from_rgb(210, 40, 40), "X"), (Color32::from_rgb(30, 150, 50), "Y"), (Color32::from_rgb(40, 70, 220), "Z")].into_iter().enumerate() {
        let mut e = centre;
        e[i] += half * 0.35;
        painter.line_segment([to_screen(cam(centre)), to_screen(cam(e))], Stroke::new(1.0, col));
        painter.text(to_screen(cam(e)), Align2::CENTER_CENTER, name, FontId::proportional(11.0), col);
    }
    // shells and wires, far to near; wires as thin quads so a shell in front tints them
    enum Item {
        Tri([V3; 3], [u8; 4]),
        Wire(V3, V3),
    }
    let mut items: Vec<(f64, Item)> = vec![];
    for sh in shells {
        for t in sh.tris {
            let c = [cam(t[0]), cam(t[1]), cam(t[2])];
            items.push(((c[0][2] + c[1][2] + c[2][2]) / 3.0, Item::Tri(c, sh.colour)));
        }
    }
    for (a, b) in &vol.wires {
        let (ca, cb) = (cam(*a), cam(*b));
        items.push(((ca[2] + cb[2]) / 2.0, Item::Wire(ca, cb)));
    }
    items.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut mesh = Mesh::default();
    let wire_col = Color32::from_rgb(25, 40, 100);
    for (_, it) in &items {
        match it {
            Item::Tri(t, c) => {
                let u = sub(t[1], t[0]);
                let w = sub(t[2], t[0]);
                let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let nl = dot(n, n).sqrt();
                let light = if nl > 0.0 { 0.6 + 0.4 * (n[2].abs() / nl) } else { 0.8 };
                let sh = |x: u8| (x as f64 * light) as u8;
                let col = Color32::from_rgba_unmultiplied(sh(c[0]), sh(c[1]), sh(c[2]), c[3]);
                let base = mesh.vertices.len() as u32;
                for p in t {
                    mesh.vertices.push(Vertex { pos: to_screen(*p), uv: WHITE_UV, color: col });
                }
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
            Item::Wire(a, b) => {
                let (pa, pb) = (to_screen(*a), to_screen(*b));
                let d = pb - pa;
                let len = d.length();
                let off = if len > 0.01 { Vec2::new(-d.y, d.x) / len * 1.2 } else { Vec2::new(1.2, 0.0) };
                let base = mesh.vertices.len() as u32;
                for pos in [pa - off, pb - off, pb + off, pa + off] {
                    mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: wire_col });
                }
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.text(
        rect.right_bottom() + Vec2::new(-8.0, -8.0),
        Align2::RIGHT_BOTTOM,
        "drag: turn   right-drag: move   wheel: zoom   double-click: reset",
        FontId::proportional(10.0),
        Color32::GRAY,
    );
}

// ------------------------------------------------------------------ vector helpers

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A field falling off as 1/r from a point: the 100 % surface is a sphere.
    #[test]
    fn surface_of_a_point_source_is_a_sphere() {
        let n = 31;
        let step = 0.2;
        let lo = [-3.0, -3.0, -3.0];
        let mut eh = vec![];
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let p = [lo[0] + i as f64 * step, lo[1] + j as f64 * step, lo[2] + k as f64 * step];
                    let r = dot(p, p).sqrt().max(0.05);
                    eh.push((28.0 * 2.0 / r, 0.0));
                }
            }
        }
        let vol = Volume { freq_mhz: 144.0, lo, step, n: [n, n, n], eh, wires: vec![([0.0; 3], [0.0, 0.0, 0.01])], ground: false, margin: 3.0 };
        let tris = surface(&vol, 1.0, (28.0, 0.073), 1.0);
        assert!(tris.len() > 100);
        for t in &tris {
            for p in t {
                let r = dot(*p, *p).sqrt();
                assert!((r - 2.0).abs() < 0.05, "{r}");
            }
        }
        let z = zone(&vol, 1.0, Standard::Icnirp1998).unwrap();
        assert!(!z.clipped);
        assert!((z.reach.unwrap() - 2.0).abs() < 0.25, "{:?}", z.reach);
    }

    #[test]
    fn margin_grows_with_power() {
        let lim = (28.0, 0.073);
        let a = auto_margin(14.0, 10.0, lim, 1.0);
        let b = auto_margin(14.0, 1000.0, lim, 1.0);
        assert!(b > a * 5.0, "{a} {b}");
        assert!(auto_margin(14.0, 10.0, lim, 0.1) > a * 3.0);
        assert!(auto_margin(0.0, 0.001, lim, 1.0) >= 1.0);
    }
}
