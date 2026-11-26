//! Bridge to the wiremom engine: MPIE with triangle basis functions and Galerkin
//! testing, perfect or Sommerfeld real ground (wires may also run in or through the
//! ground), solved in-process on the rayon pool.  The antenna is meshed with this
//! program's own segmentation; patterns, F/B and sweeps reuse the shared code in
//! `mom`, with the gain from wiremom's own far field.

use crate::model::*;
use crate::mom::{self, CalcResult, FullResult, Gain, Radiator, Solution};
use num_complex::Complex64 as C;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use wiremom::{Conductor, Ground as WGround, LoadKind, Model, Port, RealGround, Segments, Surface, TwoPortKind};

/// Sommerfeld tables shared by every calculation, so a sweep or an optimizer run
/// builds each table once (sweeps blend the tables of neighbouring frequencies).
static TABLES: LazyLock<wiremom::TableCache> = LazyLock::new(Default::default);

/// Shown when wires stop at the surface of real ground with nothing below it.  A
/// point contact into lossy earth can't be modelled (and NEC-2 gives meaningless
/// impedances for it), so, like the MININEC engine, the currents and impedance are
/// solved over perfect ground (the connection acting as a good ground system) and
/// only the radiation pattern includes the real ground.  Antennas with wires in the
/// ground (rods, buried radials) are modelled in full instead.
pub const GROUND_CONNECTED_NOTE: &str = "Wires connect to the real ground: impedance and currents are calculated over perfect ground (as for a good radial system); the pattern includes the real ground's reflection. Ground losses at the connection are not included. To model the ground system itself, add ground rods or buried radials (wires below z = 0).";

pub const SURFACE_WIRES_NOTE: &str = "Wires lying on the ground (height 0) are modelled just under the surface, in the soil, with the real ground's losses. Wires rising from height 0 keep their feed point at the surface, joined to them by a short stub in the soil.";

fn usable(w: &Wire) -> bool {
    w.length() > 0.0 && w.r != 0.0
}

/// Real ground with some wire end at ground level (after "add height").
pub fn touches_ground(ant: &Antenna) -> bool {
    let h = |p| ant.ground.height(p);
    ant.ground.kind == GroundKind::Real && ant.wires.iter().any(|w| usable(w) && (h(w.p1).abs() < 1e-6 || h(w.p2).abs() < 1e-6))
}

/// Real ground with some wire lying in the ground surface (both ends at height 0,
/// after "add height"), such as radials laid on the soil.
pub fn lies_on_ground(ant: &Antenna) -> bool {
    let h = |p| ant.ground.height(p);
    ant.ground.kind == GroundKind::Real && ant.wires.iter().any(|w| usable(w) && h(w.p1).abs() < 1e-6 && h(w.p2).abs() < 1e-6)
}

/// How far under the surface wires lying on the ground are put: 1 cm, or 5 radii
/// for thick wires (the thin-wire kernel needs the wire clear of its image).
pub fn surface_depth(ant: &Antenna) -> f64 {
    let h = |p| ant.ground.height(p);
    let r = ant.wires.iter().filter(|w| usable(w) && (h(w.p1).abs() < 1e-6 || h(w.p2).abs() < 1e-6)).map(|w| w.r.abs()).fold(0.0, f64::max);
    (5.0 * r).max(0.01)
}

/// Thin-wire engines (all of them, NEC included) aren't reliable for a wire within
/// this many outer radii of real ground's surface: the image or the soil is then
/// closer than the wire is thick, and the results jump about with tiny changes.
pub const NEAR_SURFACE_RADII: f64 = 3.0;

/// App wires that come within [`NEAR_SURFACE_RADII`] outer radii (wire plus
/// insulation) of real ground's surface, above or below it, without touching or
/// crossing it: (wire index, closest distance in m).
pub fn near_surface(ant: &Antenna) -> Vec<(usize, f64)> {
    if ant.ground.kind != GroundKind::Real {
        return vec![];
    }
    ant.wires
        .iter()
        .enumerate()
        .filter(|(_, w)| usable(w))
        .filter_map(|(i, w)| {
            let (z1, z2) = (ant.ground.height(w.p1), ant.ground.height(w.p2));
            // touching (an end at 0) or crossing: the mesher puts a node on the surface
            if z1.abs() < 1e-6 || z2.abs() < 1e-6 || z1 * z2 < 0.0 {
                return None;
            }
            let outer = w.r.abs() + if w.insulated() { w.ins_t } else { 0.0 };
            let h = z1.abs().min(z2.abs());
            (h < NEAR_SURFACE_RADII * outer).then_some((i, h))
        })
        .collect()
}

/// The warning for [`near_surface`] wires.
pub fn near_surface_note(near: &[(usize, f64)], ant: &Antenna) -> String {
    let names: Vec<String> = near.iter().map(|(i, _)| (i + 1).to_string()).collect();
    let closest = near.iter().map(|(_, h)| *h).fold(f64::INFINITY, f64::min);
    let clear = near.iter().map(|(i, _)| {
        let w = &ant.wires[*i];
        NEAR_SURFACE_RADII * (w.r.abs() + if w.insulated() { w.ins_t } else { 0.0 })
    }).fold(0.0, f64::max);
    format!(
        "Wire{} {} {} within {} wire radii of the ground surface (closest {:.1} mm). No thin-wire engine, NEC included, is reliable this close to the ground: the results can change a lot with tiny changes of height. For wires lying on the ground use height 0; otherwise keep them at least {:.0} mm clear of the surface.",
        if near.len() > 1 { "s" } else { "" },
        names.join(", "),
        if near.len() > 1 { "are" } else { "is" },
        NEAR_SURFACE_RADII,
        closest * 1000.0,
        (clear * 1000.0).ceil()
    )
}

/// Real ground with wires (partly) below the surface: modelled in full by wiremom.
pub fn has_buried(ant: &Antenna) -> bool {
    let h = |p| ant.ground.height(p);
    ant.ground.kind == GroundKind::Real && ant.wires.iter().any(|w| usable(w) && (h(w.p1) < -1e-6 || h(w.p2) < -1e-6))
}

/// The perfect-ground stand-in for wires that stop on real ground (see
/// GROUND_CONNECTED_NOTE) applies.
pub fn uses_ground_stand_in(ant: &Antenna) -> bool {
    touches_ground(ant) && !has_buried(ant) && !lies_on_ground(ant)
}

/// The wiremom model of `ant`, segmented for wavelength `lambda`, with loads
/// evaluated at `freq_mhz`; also the app wire index of each wiremom wire.
pub fn build(ant: &Antenna, lambda: f64, freq_mhz: f64) -> Result<(Model, Vec<usize>), String> {
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let conductor = match ant.material.conduction(ant.user_rho, ant.user_mu) {
        Some((sigma, mu_r)) => Conductor::Lossy { sigma, mu_r },
        None => Conductor::Perfect,
    };
    let mut wires = vec![];
    // app wire index -> wiremom wire index (zero-length / zero-radius wires are skipped,
    // as the MININEC engine does)
    let mut index = vec![None; ant.wires.len()];
    let mut app_wire = vec![];
    // wires lying on real ground go just under it (see SURFACE_WIRES_NOTE); wires
    // rising from the surface keep their end, and any feed there, in the air, and a
    // short stub straight down joins them to the wires in the soil (a feed put in the
    // soil would be shorted by it: in sea water nearly all the power went into the
    // water around the feed)
    let sink = if lies_on_ground(ant) { surface_depth(ant) } else { 0.0 };
    let at_surface = |v: f64| v.abs() < 1e-6;
    let mut stubs: Vec<([f64; 3], f64)> = vec![];
    // over sloping ground the wires are laid out in the ground frame (the surface at
    // z = 0) and turned back to world coordinates at the end, for wiremom's slope
    let frame = if grounded { ant.ground.slope().map(|s| s.frame()) } else { None };
    let local = |p: [f64; 3]| {
        let q = [p[0], p[1], p[2] + dz];
        frame.map_or(q, |f| f.to_ground(q))
    };
    for (wi, w) in ant.wires.iter().enumerate() {
        let (q1, q2) = (local(w.p1), local(w.p2));
        let (z1, z2) = (q1[2], q2[2]);
        let lying = sink > 0.0 && at_surface(z1) && at_surface(z2);
        let z = |v: f64| if lying { -sink } else { v };
        let p1 = [q1[0], q1[1], z(z1)];
        let p2 = [q2[0], q2[1], z(z2)];
        if sink > 0.0 && !lying && usable(w) {
            for p in [p1, p2] {
                let tol = 1e-6 + 1e-4 * w.length();
                if at_surface(p[2]) && !stubs.iter().any(|(q, _)| (q[0] - p[0]).hypot(q[1] - p[1]) <= tol) {
                    stubs.push((p, w.r.abs()));
                }
            }
        }
        let len = w.length();
        if !(len > 0.0) || !(w.r.abs() > 0.0) {
            continue;
        }
        // only real ground can have wires in it
        if ant.ground.kind == GroundKind::Perfect && (p1[2] < -1e-6 || p2[2] < -1e-6) {
            return Err(format!("Antenna or part of antenna is under ground. Increase height! (wire {})", wi + 1));
        }
        // wiremom's tube kernel takes segments down to half a radius, so a file's own
        // counts (EZNEC's 100 per element, say) are kept where MININEC would cut them
        let lens = mom::wire_segment_lengths_min(len, lambda, w.seg, &ant.seg, mom::WIREMOM_MIN_SEG_RADII * w.r.abs());
        index[wi] = Some(wires.len());
        app_wire.push(wi);
        wires.push(wiremom::Wire {
            start: p1,
            end: p2,
            radius: w.r.abs(),
            segments: Segments::Lengths(lens),
            conductor,
            series_l_per_m: 0.0,
            // the sleeve as such: wiremom treats it for the medium the wire is in
            // (air, or the soil for buried and surface wires)
            insulation: w.insulated().then_some(wiremom::Insulation { thickness: w.ins_t, eps_r: w.ins_eps }),
        });
    }
    // the stubs come after every app wire, so they have no entry in `app_wire`
    for (p, r) in stubs {
        wires.push(wiremom::Wire { conductor, ..wiremom::Wire::new([p[0], p[1], 0.0], [p[0], p[1], -sink], r, 1) });
    }
    if let Some(f) = frame {
        for w in &mut wires {
            w.start = f.to_world(w.start);
            w.end = f.to_world(w.end);
        }
    }
    let surfaces: Vec<Surface> = ant.plates.iter().map(|p| plate_surface(p, &wires, dz, lambda)).collect();
    let ground = match ant.ground.kind {
        GroundKind::Free => WGround::Free,
        GroundKind::Perfect => WGround::Perfect,
        // wires connected to real ground: see GROUND_CONNECTED_NOTE
        GroundKind::Real if uses_ground_stand_in(ant) => WGround::Perfect,
        GroundKind::Real => WGround::Real(RealGround { eps_r: ant.ground.eps_r, sigma: ant.ground.sigma_ms * 1e-3 }),
    };
    let mut m = Model::new(wires, ground);
    m.end_caps = !ant.open_ends;
    m.surfaces = surfaces;
    if grounded {
        m.second_ground = ant.ground.second.to_wiremom();
        m.slope = ant.ground.slope();
        m.terrain = ant.ground.terrain.to_wiremom();
        if m.slope.is_some() && m.second_ground.is_some() {
            return Err("A sloping ground can't yet be combined with a second ground: turn one of them off.".into());
        }
        if m.terrain.is_some() && m.second_ground.is_some() {
            return Err("Terrain can't be combined with a second ground: turn one of them off.".into());
        }
    }
    m.tables = TABLES.clone();
    let mesh = m.mesh().map_err(String::from)?;
    let wire_of = |w: usize, what: &str| index.get(w).copied().flatten().ok_or_else(|| format!("{what} is on wire {}, which has no length or radius", w + 1));
    for s in &ant.sources {
        let w = wire_of(s.wire, "a source")?;
        m.add_source(w, mesh.nearest_node(w, s.pos), C::from_polar(s.volts, s.phase_deg.to_radians()));
    }
    for l in &ant.loads {
        let w = wire_of(l.wire, "a load")?;
        m.add_load(w, mesh.nearest_node(w, l.pos), LoadKind::Impedance(l.impedance(freq_mhz * 1e6)));
    }
    for t in &ant.lines {
        let (wa, wb) = (wire_of(t.a_wire, "a transmission line")?, wire_of(t.b_wire, "a transmission line")?);
        let (_, ya, yb) = t.part_admittances(freq_mhz * 1e6);
        let kind = match t.network(freq_mhz * 1e6) {
            // a lumped series part or a transformer, with the shunt parts
            Some((y11, y12, y22)) => TwoPortKind::Admittance { y11, y12, y22 },
            None => TwoPortKind::Line {
                z0: t.z0.abs(),
                length: (t.length > 0.0).then_some(t.length),
                velocity_factor: t.vf,
                loss_db_per_100m: t.loss_db,
                crossed: t.reversed,
                shunt: [ya, yb],
            },
        };
        m.add_network(Port { wire: wa, node: mesh.nearest_node(wa, t.a_pos) }, Port { wire: wb, node: mesh.nearest_node(wb, t.b_pos) }, kind);
    }
    Ok((m, app_wire))
}

/// A plate's triangle mesh: grid lines about λ/10 apart (or the plate's own
/// divisions), plus lines through every wire end lying on the plate so the wire has
/// a vertex to connect to.
fn plate_surface(p: &Plate, wires: &[wiremom::Wire], dz: f64, lambda: f64) -> Surface {
    let corner = [p.corner[0], p.corner[1], p.corner[2] + dz];
    let (e1, e2) = (p.edge1, p.edge2);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let len = |a: [f64; 3]| dot(a, a).sqrt();
    // plate coordinates (u, v, w) of a point: solve the 2x2 normal equations
    let (g11, g12, g22) = (dot(e1, e1), dot(e1, e2), dot(e2, e2));
    let det = g11 * g22 - g12 * g12;
    let mut us = vec![0.0, 1.0];
    let mut vs = vec![0.0, 1.0];
    let tol = 1e-6 * (len(e1) + len(e2));
    for w in wires {
        for q in [w.start, w.end] {
            let d = [q[0] - corner[0], q[1] - corner[1], q[2] - corner[2]];
            let (b1, b2) = (dot(d, e1), dot(d, e2));
            let (u, v) = ((g22 * b1 - g12 * b2) / det, (g11 * b2 - g12 * b1) / det);
            let off = [0, 1, 2].map(|i| d[i] - u * e1[i] - v * e2[i]);
            if len(off) < tol && (-1e-9..=1.0 + 1e-9).contains(&u) && (-1e-9..=1.0 + 1e-9).contains(&v) {
                us.push(u.clamp(0.0, 1.0));
                vs.push(v.clamp(0.0, 1.0));
            }
        }
    }
    let lines = |mut xs: Vec<f64>, edge: f64, n: usize| -> Vec<f64> {
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let step = if n > 0 { 1.0 / n as f64 } else { (lambda / 10.0 / edge.max(1e-12)).min(1.0) };
        let mut out = vec![xs[0]];
        for w in xs.windows(2) {
            let k = ((w[1] - w[0]) / step - 1e-9).ceil().max(1.0) as usize;
            for i in 1..=k {
                out.push(w[0] + (w[1] - w[0]) * i as f64 / k as f64);
            }
        }
        out
    };
    Surface::plate_grid(corner, e1, e2, &lines(us, len(e1), p.n1), &lines(vs, len(e2), p.n2))
}

/// Build the Sommerfeld tables a sweep over `freqs` (MHz) will need, in parallel,
/// so the per-frequency solves all find them ready.
pub fn prepare_sweep(ant: &Antenna, freqs_mhz: &[f64], seg_freq_mhz: f64) {
    if ant.ground.kind != GroundKind::Real {
        return;
    }
    if let Ok((m, _)) = build(ant, 299.792458 / seg_freq_mhz, seg_freq_mhz) {
        let hz: Vec<f64> = freqs_mhz.iter().map(|f| f * 1e6).collect();
        // errors (e.g. wires under ground) are reported by the solves themselves
        let _ = TABLES.prepare(&m, &hz);
    }
}

/// Solve at one frequency; returns the summary, the solution in this program's form,
/// the radiator (when `need_far`), the engine's own solution and the app wire index
/// of each of its wires.
pub fn quick(ant: &Antenna, freq_mhz: f64, seg_freq_mhz: f64, need_far: bool) -> Result<(CalcResult, Solution, Option<Radiator>, Arc<wiremom::Solution>, Vec<usize>), String> {
    let t0 = Instant::now();
    if ant.sources.is_empty() {
        return Err("The antenna has no source.".into());
    }
    let (model, app_wire) = build(ant, 299.792458 / seg_freq_mhz, freq_mhz)?;
    let mut ws = model.solve(freq_mhz * 1e6).map_err(String::from)?;
    if uses_ground_stand_in(ant) {
        // currents over perfect ground, pattern with the real ground's reflection
        ws.ground = WGround::Real(RealGround { eps_r: ant.ground.eps_r, sigma: ant.ground.sigma_ms * 1e-3 });
    }
    let ws = Arc::new(ws);
    let sol = Solution {
        freq_mhz,
        currents: vec![],
        z_in: ws.source_impedances(),
        p_in: ws.input_power(),
        losses: [ws.losses.wire, ws.losses.loads],
        n_unknowns: ws.mesh.n_unknowns(),
        n_segs: ws.mesh.n_segments(),
        t_fill_ms: ws.t_fill_ms,
        t_solve_ms: ws.t_solve_ms,
    };
    let rad = need_far.then(|| {
        // wiremom's own far field: it also covers currents in the ground
        let w = ws.clone();
        let gain: mom::GainFn = Arc::new(move |az: f64, el: f64| {
            let g = w.gain((90.0 - el).to_radians(), az.to_radians());
            Gain { total: g.total, hor: g.phi, ver: g.theta }
        });
        Radiator::from_fn(freq_mhz, &ant.ground, gain)
    });
    let res = mom::summarize(ant, freq_mhz, t0, &sol, rad.as_ref());
    Ok((res, sol, rad, ws, app_wire))
}

/// Full calculation for the Calculate tab.
pub fn calculate(ant: &Antenna, prog: &crate::progress::Progress) -> Result<FullResult, String> {
    let (mut res, _, rad, ws, app_wire) = quick(ant, ant.freq_mhz, ant.freq_mhz, true)?;
    let patterns = mom::patterns(ant, &mut res, &rad.unwrap(), prog)?;
    // segmentation warnings from this program's mesher (it refuses buried wires,
    // which only this engine models)
    // (MININEC's diameter rule doesn't apply here: wiremom gets its own segmentation)
    let mut warnings: Vec<String> =
        mom::build_mesh(ant, ant.lambda()).map(|m| m.warnings).unwrap_or_default().into_iter().filter(|w| !w.ends_with(mom::SHORT_SEGMENTS_NOTE)).collect();
    let lambda = ant.lambda();
    for (wi, w) in ant.wires.iter().enumerate() {
        let (len, r) = (w.length(), w.r.abs());
        if len > 0.0 && r > 0.0 {
            let lens = mom::wire_segment_lengths_min(len, lambda, w.seg, &ant.seg, mom::WIREMOM_MIN_SEG_RADII * r);
            if lens.iter().any(|l| *l < mom::WIREMOM_MIN_SEG_RADII * r * 0.999) {
                warnings.push(format!("Wire {}: segments are shorter than half the wire radius (the wire is too short for two longer ones).", wi + 1));
            }
        }
    }
    warnings.extend(ws.warnings.iter().cloned());
    if uses_ground_stand_in(ant) {
        warnings.push(GROUND_CONNECTED_NOTE.to_string());
    }
    if lies_on_ground(ant) {
        warnings.push(SURFACE_WIRES_NOTE.to_string());
    }
    let mesh = view_mesh(ant, &ws, &app_wire);
    let seg_currents = ws.segment_currents().into_iter().map(|(a, b, i)| (a, b, i.norm())).collect();
    let l = ws.losses;
    let mut losses = vec![("Wires", l.wire)];
    if !ant.loads.is_empty() {
        losses.push(("Loads", l.loads));
    }
    if !ant.lines.is_empty() {
        losses.push(("Lines", l.networks));
    }
    let power = mom::PowerBudget::new(ant, &patterns, ws.input_power(), &losses, uses_ground_stand_in(ant));
    res.eff = power.radiated;
    let near = crate::nearfield::Currents { filaments: near_filaments(&ws), p_in: ws.input_power(), plates_left_out: !ant.plates.is_empty() };
    Ok(FullResult { result: res, patterns, mesh, currents: vec![], warnings, seg_currents: Some(seg_currents), power, near })
}

/// The segment layout for the 3D view, from wiremom's mesh, one entry per app wire.
/// wiremom's currents run linearly along each segment: I = rise·u/L + fall·(1 − u/L).
fn near_filaments(ws: &wiremom::Solution) -> Vec<crate::nearfield::Filament> {
    use wiremom::mesh::{FALL, RISE};
    let m = &ws.mesh;
    let mut amp = vec![[num_complex::Complex64::new(0.0, 0.0); 2]; m.n_segments()];
    for (b, &i) in m.basis.iter().zip(&ws.currents) {
        for h in &b.halves {
            amp[h.seg][h.shape] += i * h.c;
        }
    }
    (0..m.n_segments())
        .map(|s| crate::nearfield::Filament { a: world(ws, m.p0[s]), b: world(ws, m.p1[s]), ia: amp[s][FALL], ib: amp[s][RISE], r: m.radius[s] })
        .collect()
}

/// A point of wiremom's mesh in world coordinates (the mesh is in the ground frame
/// over sloping ground).
fn world(ws: &wiremom::Solution, p: [f64; 3]) -> [f64; 3] {
    ws.frame.map_or(p, |f| f.to_world(p))
}

fn view_mesh(ant: &Antenna, ws: &wiremom::Solution, app_wire: &[usize]) -> mom::Mesh {
    let m = &ws.mesh;
    let mut segs = vec![];
    let mut wire_segs = vec![(0, 0); ant.wires.len()];
    for (wi, list) in m.wire_segs.iter().enumerate() {
        // the stubs joining rising wires to surface wires (see `build`) aren't shown
        let Some(&aw) = app_wire.get(wi) else { continue };
        wire_segs[aw] = (segs.len(), list.len());
        for &s in list {
            segs.push(mom::Seg { a: world(ws, m.p0[s]), b: world(ws, m.p1[s]), r: m.radius[s], wire: aw });
        }
    }
    mom::Mesh { segs, bases: vec![], wire_gaps: vec![], wire_segs, warnings: vec![], image: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insulation reaches wiremom for wires in the soil: on four radials lying on
    /// average ground it changes the result (a little; see wiremom's tests/insulated.rs
    /// for the physics), and the power still balances.
    /// The antenna turned by hand into the slope's ground frame, over level ground.
    fn turned_by_hand(a: &Antenna) -> Antenna {
        let f = a.ground.slope().unwrap().frame();
        let mut b = a.clone();
        let dz = a.ground.add_height;
        for w in &mut b.wires {
            for p in [&mut w.p1, &mut w.p2] {
                *p = f.to_ground([p[0], p[1], p[2] + dz]);
            }
        }
        b.ground.add_height = 0.0;
        b.ground.slope_deg = 0.0;
        b
    }

    /// A vertical with four radials lying on sloping real ground (ends on the tilted
    /// surface): wireant finds them on the ground, puts them in the soil square to the
    /// surface, and gets what the same antenna turned by hand gets over level ground.
    #[test]
    fn radials_on_sloping_ground() {
        let mut a = Antenna::example(3);
        a.engine = Engine::Wiremom;
        a.ground.kind = GroundKind::Real;
        a.ground.slope_deg = 12.0;
        a.ground.downhill_deg = 40.0;
        let l = a.wires[0].length();
        for k in 0..4 {
            let t = k as f64 * std::f64::consts::FRAC_PI_2 + 0.3;
            let (x, y) = (l * t.cos(), l * t.sin());
            let mut w = Wire::new([0.0, 0.0, 0.0], [x, y, a.ground.surface_z(x, y)], 0.001);
            w.seg = -1;
            a.wires.push(w);
        }
        assert!(lies_on_ground(&a) && touches_ground(&a) && !has_buried(&a));
        let s = mom::calculate(&a).unwrap();
        let flat = mom::calculate(&turned_by_hand(&a)).unwrap();
        println!("sloping {:.3} eff {:.3} | turned {:.3} eff {:.3}", s.result.z, s.result.eff, flat.result.z, flat.result.eff);
        assert!((s.result.z - flat.result.z).norm() < 1e-6 * flat.result.z.norm());
        assert!((s.power.ground.unwrap() - flat.power.ground.unwrap()).abs() < 0.01);
        // the mesh shown in the 3D view is back in world coordinates
        let seg = &s.mesh.segs[s.mesh.wire_segs[1].0];
        // (the radial runs 1 cm into the soil, square to the sloping surface)
        let depth = a.ground.height(seg.a);
        assert!((depth + surface_depth(&a)).abs() < 1e-9 && seg.a[0].hypot(seg.a[1]) < 0.01, "{:?} {depth}", seg.a);
    }

    /// A dipole on a hillside: the pattern reaches below the horizon downhill and its
    /// peak drops toward it; level ground and the other engines are unchanged.
    #[test]
    fn dipole_on_a_hillside() {
        // 20 m dipole along Y, 10 m up, the slope falling toward +X (broadside)
        let mut a = with(0, Engine::Wiremom);
        a.wires[0].p1 = [0.0, -5.2, 10.0];
        a.wires[0].p2 = [0.0, 5.2, 10.0];
        a.ground.kind = GroundKind::Real;
        a.ground.add_height = 0.0;
        let level = mom::calculate(&a).unwrap();
        a.ground.downhill_deg = 0.0;
        a.ground.slope_deg = 0.0;
        let zero = mom::calculate(&a).unwrap();
        assert!((zero.result.z - level.result.z).norm() < 1e-9);
        a.ground.slope_deg = 10.0;
        let hill = mom::calculate(&a).unwrap();
        println!("level el {:.1} Ga {:.2} | 10° slope el {:.1} Ga {:.2} eff {:.3}", level.result.el_max, level.result.ga_dbi, hill.result.el_max, hill.result.ga_dbi, hill.result.eff);
        assert!(hill.result.el_max < level.result.el_max - 5.0);
        assert!(hill.patterns.grid_els[0] <= -9.0 && (hill.patterns.cut_tilt - 10.0).abs() < 1e-6);
        let cut = hill.patterns.el_cut_above_ground();
        assert!(cut.first().unwrap().0 <= -9.9 && cut.iter().any(|(t, g)| *t < -1.0 && g.total > 0.0));
        assert!(hill.result.eff > 0.3 && hill.result.eff < 1.0 && hill.patterns.el_beamwidth().is_some());
        // a 10° slope square to the cut tilts nothing in it
        let mut side = a.clone();
        side.ground.downhill_deg += 90.0;
        assert!(Ground::tilt_toward(&side.ground, a.ground.downhill_deg).abs() < 1e-9);
        for e in [Engine::Mininec, Engine::Nec2] {
            let mut b = a.clone();
            b.engine = e;
            assert_eq!(mom::calculate(&b).err().as_deref(), Some(mom::SLOPE_ENGINE_NOTE));
        }
        // the slope is kept in both file formats
        let m = crate::maa::parse(&crate::maa::write(&a)).unwrap();
        let n = crate::nec_file::parse(&crate::nec_file::write(&a), "x").unwrap().ant;
        for g in [m.ground, n.ground] {
            assert_eq!((g.slope_deg, g.downhill_deg), (a.ground.slope_deg, a.ground.downhill_deg));
        }
    }

    /// Terrain shapes the patterns, not the impedance: level terrain changes nothing,
    /// a hilltop lowers the takeoff angle, the other engines refuse it, and both file
    /// formats keep it.
    #[test]
    fn terrain_around_a_dipole() {
        let mut a = with(0, Engine::Wiremom);
        a.wires[0].p1 = [0.0, -5.2, 10.0];
        a.wires[0].p2 = [0.0, 5.2, 10.0];
        a.ground.kind = GroundKind::Real;
        a.ground.add_height = 0.0;
        let level = mom::calculate(&a).unwrap();
        a.ground.terrain = TerrainSpec { enabled: true, profiles: vec![TerrainProfile { bearing_deg: 0.0, points: vec![[0.0, 0.0], [700.0, 0.0]] }] };
        let flat = mom::calculate(&a).unwrap();
        assert!((flat.result.z - level.result.z).norm() < 1e-9 && (flat.result.ga_dbi - level.result.ga_dbi).abs() < 1e-6);
        // turned off, it is ignored
        a.ground.terrain.enabled = false;
        assert!((mom::calculate(&a).unwrap().result.ga_dbi - level.result.ga_dbi).abs() < 1e-9);
        // a hilltop: 100 m down over the first km toward +X, level the other way
        a.ground.terrain = TerrainSpec {
            enabled: true,
            profiles: vec![
                TerrainProfile { bearing_deg: 0.0, points: vec![[0.0, 0.0], [50.0, 0.0], [1000.0, -100.0]] },
                TerrainProfile { bearing_deg: 180.0, points: vec![[0.0, 0.0]] },
            ],
        };
        let top = mom::calculate(&a).unwrap();
        println!("level el {:.1} Ga {:.2} | hilltop el {:.1} Ga {:.2}", level.result.el_max, level.result.ga_dbi, top.result.el_max, top.result.ga_dbi);
        assert!((top.result.z - level.result.z).norm() < 1e-9);
        // 3° toward +X (downhill), from the 3D grid (its rows are 3° apart from 0°,
        // azimuth 0 first)
        let low = |c: &mom::FullResult| c.patterns.grid[c.patterns.grid_els.iter().position(|e| (*e - 3.0).abs() < 1e-9).unwrap()][0];
        assert!(10.0 * (low(&top) / low(&level)).log10() > 8.0, "3° downhill gains");
        assert!(top.patterns.grid_els[0] >= 0.0);
        for e in [Engine::Mininec, Engine::Nec2] {
            let mut b = a.clone();
            b.engine = e;
            assert_eq!(mom::calculate(&b).err().as_deref(), Some(mom::TERRAIN_ENGINE_NOTE));
        }
        let m = crate::maa::parse(&crate::maa::write(&a)).unwrap();
        let n = crate::nec_file::parse(&crate::nec_file::write(&a), "x").unwrap().ant;
        assert_eq!(m.ground.terrain, a.ground.terrain);
        assert_eq!(n.ground.terrain, a.ground.terrain);
        // with a second ground it is refused
        a.ground.second.enabled = true;
        assert!(mom::calculate(&a).is_err());
    }

    #[test]
    fn terrain_profile_import() {
        let text = "# exported profile\ndist,elev\n0, 120\n250 , 130\n500;125\n\n1000\t60 extra\n";
        let p = TerrainProfile::parse_points(text, false);
        assert_eq!(p, vec![[0.0, 0.0], [250.0, 10.0], [500.0, 5.0], [1000.0, -60.0]]);
        let f = TerrainProfile::parse_points("0 0\n1000 100\n", true);
        assert!((f[1][0] - 304.8).abs() < 1e-9 && (f[1][1] - 30.48).abs() < 1e-9);
    }

    #[test]
    fn insulated_surface_radials() {
        let mut a = Antenna::example(3);
        a.engine = Engine::Wiremom;
        a.ground.kind = GroundKind::Real;
        let l = a.wires[0].length();
        for k in 0..4 {
            let t = k as f64 * std::f64::consts::FRAC_PI_2;
            let mut w = Wire::new([0.0, 0.0, 0.0], [l * t.cos(), l * t.sin(), 0.0], 0.001);
            w.seg = -1;
            a.wires.push(w);
        }
        let bare = mom::calculate(&a).unwrap();
        for w in &mut a.wires[1..] {
            w.ins_t = 0.001;
            w.ins_eps = 3.5;
        }
        let ins = mom::calculate(&a).unwrap();
        println!("bare {:.2} {} | insulated {:.2} {}", bare.result.z, bare.power.line(), ins.result.z, ins.power.line());
        assert!((ins.result.z - bare.result.z).norm() > 0.5, "insulation must change something");
        assert!(ins.power.ground.unwrap() > 0.2 && ins.result.eff.is_finite());
    }

    fn with(i: usize, e: Engine) -> Antenna {
        let mut a = Antenna::example(i);
        a.engine = e;
        a
    }

    fn nec2(a: &Antenna) -> CalcResult {
        let mut n = a.clone();
        n.engine = Engine::Nec2;
        mom::calculate(&n).unwrap().result
    }

    #[test]
    fn examples_agree_with_nec2_and_mininec() {
        for i in 0..Antenna::example_names().len() {
            let w = mom::calculate(&with(i, Engine::Wiremom)).unwrap().result;
            let name = Antenna::example(i).name;
            if !Antenna::example(i).lines.is_empty() {
                // MININEC has no transformers or lines: NEC-2 only
                if crate::nec::available() {
                    // NEC-2 can't model a wire into lossy ground (GROUND_CONNECTED_NOTE): compare over perfect ground
                    let mut a = with(i, Engine::Wiremom);
                    if touches_ground(&a) {
                        a.ground.kind = GroundKind::Perfect;
                    }
                    let w = mom::calculate(&a).unwrap().result;
                    let n = nec2(&a);
                    println!("{name}: wiremom Z={:.1} Ga={:.2} | NEC-2 Z={:.1} Ga={:.2}", w.z, w.ga_dbi, n.z, n.ga_dbi);
                    // (an end feed's high impedance makes NEC-2 sensitive to the short gap segments)
                    assert!((w.z - n.z).norm() < 0.15 * n.z.norm(), "{name}: Z {} vs NEC-2 {}", w.z, n.z);
                }
                continue;
            }
            let m = mom::calculate(&with(i, Engine::Mininec)).unwrap().result;
            print!("{name}: wiremom Z={:.1} Ga={:.2} FB={:.1} ({:.0} ms) | MININEC Z={:.1} Ga={:.2} FB={:.1}", w.z, w.ga_dbi, w.fb_db, w.t_total_ms, m.z, m.ga_dbi, m.fb_db);
            if crate::nec::available() {
                let n = nec2(&with(i, Engine::Nec2));
                println!(" | NEC-2 Z={:.1} Ga={:.2} FB={:.1}", n.z, n.ga_dbi, n.fb_db);
                assert!((w.ga_dbi - n.ga_dbi).abs() < 0.3, "{name}: gain {} vs NEC-2 {}", w.ga_dbi, n.ga_dbi);
                assert!((w.z - n.z).norm() < 0.08 * n.z.norm() + 2.0, "{name}: Z {} vs NEC-2 {}", w.z, n.z);
            } else {
                println!();
            }
            assert!((w.ga_dbi - m.ga_dbi).abs() < 1.5);
        }
    }

    #[test]
    fn real_ground_matches_nec2() {
        if !crate::nec::available() {
            return;
        }
        for i in 0..Antenna::example_names().len() {
            let mut a = with(i, Engine::Wiremom);
            if a.ground.kind == GroundKind::Free {
                continue;
            }
            a.ground.kind = GroundKind::Real;
            a.ground.eps_r = 13.0;
            a.ground.sigma_ms = 5.0;
            if touches_ground(&a) {
                continue; // NEC-2 can't model a connection into lossy ground (see GROUND_CONNECTED_NOTE)
            }
            let w = mom::calculate(&a);
            let n = nec2(&a);
            let w = w.unwrap().result;
            println!("{} over average ground: wiremom Z={:.1} Ga={:.2} el={:.0} | NEC-2 Z={:.1} Ga={:.2} el={:.0}", a.name, w.z, w.ga_dbi, w.el_max, n.z, n.ga_dbi, n.el_max);
            assert!((w.ga_dbi - n.ga_dbi).abs() < 0.3, "{}", a.name);
            assert!((w.z - n.z).norm() < 0.08 * n.z.norm() + 2.0, "{}", a.name);
        }
    }

    #[test]
    fn ground_connected_vertical_over_real_ground() {
        // impedance as over perfect ground, pattern with real-ground reflection: the
        // same model as the MININEC engine, so the two must agree closely
        let mut a = with(3, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        assert!(uses_ground_stand_in(&a));
        let w = mom::calculate(&a).unwrap();
        assert!(w.warnings.iter().any(|s| s == GROUND_CONNECTED_NOTE));
        let mut p = a.clone();
        p.ground.kind = GroundKind::Perfect;
        let zp = mom::calculate(&p).unwrap().result.z;
        assert!((w.result.z - zp).norm() < 1e-9 * zp.norm());
        a.engine = Engine::Mininec;
        let m = mom::calculate(&a).unwrap().result;
        println!("vertical on average ground: wiremom Ga={:.2} el={} | MININEC Ga={:.2} el={}", w.result.ga_dbi, w.result.el_max, m.ga_dbi, m.el_max);
        assert!((w.result.ga_dbi - m.ga_dbi).abs() < 0.1);
        // real ground pulls the lobe up off the horizon and costs gain
        assert!(w.result.el_max > 5.0 && w.result.ga_dbi < 2.0);
    }

    #[test]
    fn radials_lying_on_real_ground_are_modelled_in_the_soil() {
        // a vertical with radials laid on the ground, all to one side: at height 0
        // the radials must still count (the perfect-ground stand-in used to cancel
        // them, giving a symmetric pattern)
        let mut a = with(3, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        for i in 0..6 {
            let t = (-50.0 + 20.0 * i as f64).to_radians();
            a.wires.push(Wire::new([0.0, 0.0, 0.0], [8.0 * t.cos(), 8.0 * t.sin(), 0.0], 0.001));
        }
        assert!(lies_on_ground(&a) && !uses_ground_stand_in(&a));
        let w = mom::calculate(&a).unwrap();
        assert!(w.warnings.iter().any(|s| s == SURFACE_WIRES_NOTE));
        assert!(!w.warnings.iter().any(|s| s == GROUND_CONNECTED_NOTE));
        let az = &w.patterns.az_cut;
        let db = |deg: usize| 10.0 * az[deg].1.total.log10();
        println!("radials on the ground to +x: Z={:.1} Ga={:.2}, gain toward 0 deg {:.2}, 180 deg {:.2}", w.result.z, w.result.ga_dbi, db(0), db(180));
        assert!((db(0) - db(180)).abs() > 0.5, "{} vs {}", db(0), db(180));
        // ground loss: well under the perfect-ground gain
        assert!(w.result.ga_dbi < 3.0);
    }

    /// The 28 MHz ground plane with its radials flat at `h` over real ground.
    fn flat_ground_plane(h: f64, eps_r: f64, sigma_ms: f64) -> Antenna {
        let mut a = with(4, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = eps_r;
        a.ground.sigma_ms = sigma_ms;
        a.ground.add_height = h;
        for w in a.wires.iter_mut().skip(1) {
            w.p2[2] = 0.0;
        }
        a
    }

    #[test]
    fn surface_radials_keep_the_feed_out_of_the_soil() {
        // radials lying on sea water: the feed at the foot of the vertical used to go
        // 1 cm into the water with them, which shorted it (about 2 ohms, 7 % radiated);
        // now it stays at the surface and the result is in line with radials 1 cm up
        let at = |h: f64| mom::calculate(&flat_ground_plane(h, 81.0, 5000.0)).unwrap();
        let (on, up) = (at(0.0), at(0.01));
        println!("sea: on the surface Z={:.1} {:.1} %, 1 cm up Z={:.1} {:.1} %", on.result.z, on.power.radiated * 100.0, up.result.z, up.power.radiated * 100.0);
        assert!(on.result.z.re > 30.0 && on.result.z.re < 60.0, "{}", on.result.z);
        assert!(on.power.radiated > 0.6 && (on.power.radiated - up.power.radiated).abs() < 0.1);
        // the stub joining the vertical to the radials isn't an app wire
        assert_eq!(on.mesh.wire_segs.len(), 5);
    }

    #[test]
    fn wires_hugging_the_surface_are_flagged() {
        // 2 mm radius radials: 3 radii is 6 mm
        let note = |h: f64| {
            let a = flat_ground_plane(h, 13.0, 5.0);
            let near = near_surface(&a);
            (near.len(), mom::calculate(&a).unwrap().warnings.iter().any(|w| w.contains("thin-wire engine")))
        };
        // the radials and the foot of the vertical
        assert_eq!(note(0.003), (5, true));
        // just under the surface too (the vertical crosses it, which is fine)
        assert_eq!(note(-0.003), (4, true));
        assert_eq!(note(0.01), (0, false));
        assert_eq!(note(0.0), (0, false));
    }

    #[test]
    fn second_ground_agrees_across_engines() {
        // ground plane at 28 MHz, sea 15 m away toward 60°: NEC-2's reflection-point
        // rule in wiremom vs nec2c (rotated deck), and MININEC/wiremom with the
        // Fresnel zone
        let mut a = with(4, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.add_height = 5.0;
        a.ground.second = crate::model::SecondMedium { enabled: true, distance: 15.0, bearing_deg: 60.0, fresnel: false, ..Default::default() };
        let grid = |e: Engine, fresnel: bool| {
            let mut b = a.clone();
            b.engine = e;
            b.ground.second.fresnel = fresnel;
            let r = mom::calculate(&b).unwrap();
            let p = r.patterns;
            (p.grid, p.grid_els, p.grid_step, r.result)
        };
        let (gw, els, step, rw) = grid(Engine::Wiremom, false);
        let (gn, els_n, step_n, rn) = grid(Engine::Nec2, false);
        assert_eq!((els.len(), step), (els_n.len(), step_n));
        let db = |x: f64| 10.0 * x.max(1e-30).log10();
        let mut worst = 0.0f64;
        for (i, &el) in els.iter().enumerate() {
            if el < 3.0 || el > 60.0 {
                continue;
            }
            for j in 0..gw[i].len() {
                let (x, y) = (db(gw[i][j]), db(gn[i][j]));
                if y > -10.0 {
                    worst = worst.max((x - y).abs());
                }
            }
        }
        println!("sea at 60°: wiremom Ga {:.2} az {} el {} | NEC-2 Ga {:.2} az {} el {} | worst grid difference {worst:.2} dB", rw.ga_dbi, rw.az_max, rw.el_max, rn.ga_dbi, rn.az_max, rn.el_max);
        assert!(worst < 1.0, "{worst}");
        // Fresnel-zone mode: MININEC and wiremom share the weighting
        let (gf, ..) = grid(Engine::Wiremom, true);
        let (gm, ..) = grid(Engine::Mininec, true);
        let i15 = els.iter().position(|&e| (e - 15.0).abs() < 1.6).unwrap();
        let j60 = (60.0 / step).round() as usize;
        let j240 = (240.0 / step).round() as usize;
        let fb = |g: &Vec<Vec<f64>>| db(g[i15][j60]) - db(g[i15][j240]);
        println!("15° el, toward sea minus away: wiremom {:.2} dB, MININEC {:.2} dB, NEC-2 rule {:.2}, nec2c {:.2}", fb(&gf), fb(&gm), fb(&gw), fb(&gn));
        assert!(fb(&gw) > 1.0 && fb(&gn) > 1.0);
        assert!(fb(&gf) > 1.0 && fb(&gm) > 1.0);
        assert!((fb(&gf) - fb(&gm)).abs() < 1.0);
    }

    #[test]
    fn vertical_with_rod_and_buried_radials() {
        // the quarter-wave vertical fed against a 1 m rod and 8 buried radials over
        // average ground: modelled in full (no stand-in), with ground loss
        let mut a = with(3, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 0.005));
        for i in 0..8 {
            let t = 2.0 * std::f64::consts::PI * i as f64 / 8.0;
            a.wires.push(Wire::new([0.0, 0.0, -0.1], [8.0 * t.cos(), 8.0 * t.sin(), -0.1], 0.002));
        }
        // radials join the rod 0.1 m down
        a.wires[1] = Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, -0.1], 0.005);
        a.wires.insert(2, Wire::new([0.0, 0.0, -0.1], [0.0, 0.0, -1.0], 0.005));
        assert!(has_buried(&a) && !uses_ground_stand_in(&a));
        let w = mom::calculate(&a).unwrap();
        let r = &w.result;
        println!("vertical + rod + 8 buried radials: Z={:.1} Ga={:.2} el={} warnings {:?}", r.z, r.ga_dbi, r.el_max, w.warnings);
        assert!(!w.warnings.iter().any(|s| s == GROUND_CONNECTED_NOTE));
        // ground loss: well below the perfect-ground 5.1 dBi, lobe off the horizon
        assert!(r.ga_dbi < 2.0 && r.ga_dbi > -6.0, "{}", r.ga_dbi);
        assert!(r.z.re > 30.0 && r.z.re < 120.0, "{}", r.z);
        // the 3D view gets every wire's segments
        assert_eq!(w.mesh.wire_segs.len(), a.wires.len());
        assert!(w.mesh.wire_segs.iter().all(|&(_, n)| n > 0));
    }

    #[test]
    fn dipole_fed_through_transformer() {
        // the half-wave dipole through 4:1 at its source: a quarter of its impedance
        // (ideal), and the circuit formula's value for a real core
        let d = with(0, Engine::Wiremom);
        let zd = mom::calculate(&d).unwrap().result.z;
        for x in [Transformer { ratio: 4.0, ..Transformer::default() }, Transformer { ratio: 4.0, l_uh: 3.0, k: 0.98, q: 5.0 }] {
            let mut a = d.clone();
            crate::matching::feed_through_transformer(&mut a, 0, x).unwrap();
            let r = mom::calculate(&a).unwrap().result;
            let (y11, y12, y22) = x.admittance(a.freq_mhz * 1e6, false);
            let want = 1.0 / (y11 - y12 * y12 / (y22 + 1.0 / zd));
            print!("{x:?}: dipole {zd:.2}, through it {:.2} (formula {want:.2}), eff {:.3}", r.z, r.eff);
            assert!((r.z - want).norm() < 0.02 * want.norm());
            if crate::nec::available() {
                a.engine = Engine::Nec2;
                let n = mom::calculate(&a).unwrap().result;
                print!(", NEC-2 {:.2}", n.z);
                assert!((r.z - n.z).norm() < 0.05 * n.z.norm(), "{} vs {}", r.z, n.z);
            }
            println!();
        }
    }

    #[test]
    fn coax_choke_stops_common_mode_current() {
        // a dipole fed off-centre (asymmetric) with a vertical feed-line wire hanging from
        // the feed: the line's shield carries common-mode current until a choke stops it
        let mut a = with(0, Engine::Wiremom);
        a.ground = Ground { kind: GroundKind::Real, add_height: 0.0, ..a.ground.clone() };
        a.wires = vec![Wire::new([0.0, -3.4, 10.0], [0.0, 0.0, 10.0], 1e-3), Wire::new([0.0, 0.0, 10.0], [0.0, 6.9, 10.0], 1e-3), Wire::new([0.0, 0.0, 10.0], [0.0, 0.0, 0.0], 2.5e-3)];
        a.sources = vec![Source { wire: 0, pos: 1.0, ..Source::default() }];
        let shield = |a: &Antenna| {
            let full = mom::calculate(a).unwrap();
            let i_feed = full.result.z.inv().norm();
            // segments of the vertical wire (on the z axis), below the feed point
            let on_line = |p: [f64; 3]| p[0].abs() < 1e-6 && p[1].abs() < 1e-6 && p[2] < 9.5;
            let m = full.seg_currents.unwrap().iter().filter(|(a, b, _)| on_line(*a) && on_line(*b)).map(|s| s.2).fold(0.0, f64::max);
            m / i_feed
        };
        let without = shield(&a);
        let f0 = a.freq_mhz;
        // 8 turns on a former sized to resonate near 14 MHz
        let c = Choke { turns: 8.0, form_d: 0.15, coax_d: 0.005, length: 0.0, q: 20.0 };
        a.loads.push(Load { wire: 2, pos: 0.05, kind: crate::model::LoadKind::Choke(c) });
        let with_choke = shield(&a);
        println!("{} at {f0} MHz: shield current {without:.3} → {with_choke:.3} of the feed current", c.describe());
        assert!(with_choke < 0.5 * without);
    }

    #[test]
    fn dipole_fed_through_lines_matches_nec2() {
        // the half-wave dipole fed from a short feed wire through 75 ohm lines
        for (len, vf) in [(5.3, 1.0), (9.1, 0.66)] {
            let mut a = with(0, Engine::Wiremom);
            a.wires.push(Wire::new([0.0, 0.0, 20.0], [0.0, 0.0, 20.05], 0.001));
            a.wires[1].seg = 3;
            a.sources = vec![Source { wire: 1, pos: 0.5, phase_deg: 0.0, volts: 1.0, current: false }];
            let mut t = TLine::new(1, 0.5, 0, 0.5);
            t.z0 = 75.0;
            t.length = len;
            t.vf = vf;
            a.lines.push(t);
            let w = mom::calculate(&a).unwrap().result;
            let mut d = with(0, Engine::Wiremom);
            d.material = a.material;
            let zd = mom::calculate(&d).unwrap().result.z;
            let bl = 2.0 * std::f64::consts::PI * len / (vf * a.lambda());
            let want = 75.0 * (zd + C::new(0.0, 75.0 * bl.tan())) / (75.0 + C::new(0.0, 1.0) * zd * bl.tan());
            print!("line {len} m vf {vf}: wiremom {:.2}, line formula {:.2}", w.z, want);
            assert!((w.z - want).norm() < 0.02 * want.norm());
            if crate::nec::available() {
                a.engine = Engine::Nec2;
                let n = mom::calculate(&a).unwrap().result;
                print!(", NEC-2 {:.2}", n.z);
                assert!((w.z - n.z).norm() < 0.05 * n.z.norm(), "{} vs {}", w.z, n.z);
            }
            println!();
        }
    }

    #[test]
    fn monopole_fed_against_a_plate() {
        // a quarter-wave monopole on a 1 x 1 wavelength plate at 300 MHz (the plate's
        // grid gets a line through the monopole's base automatically)
        let mut a = with(0, Engine::Wiremom);
        a.freq_mhz = 299.792458;
        a.material = Material::Lossless;
        a.wires = vec![Wire::new([0.1, 0.2, 0.0], [0.1, 0.2, 0.24], 0.002)];
        a.wires[0].seg = 12;
        a.sources = vec![Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0, current: false }];
        a.plates = vec![Plate::new([-0.5, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])];
        let r = mom::calculate(&a).unwrap().result;
        println!("monopole on a plate: Z={:.2} Ga={:.2} el={}", r.z, r.ga_dbi, r.el_max);
        assert!(r.z.re > 33.0 && r.z.re < 48.0, "{}", r.z);
        // other engines refuse plates
        a.engine = Engine::Mininec;
        assert!(mom::calculate(&a).is_err());
    }

    #[test]
    fn sweep_runs_in_parallel() {
        let mut a = with(2, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        if a.ground.add_height <= 0.0 {
            a.ground.add_height = 10.0;
        }
        let r = mom::sweep(&a, 13.8, 14.5, 24, true).unwrap();
        println!("wiremom real-ground sweep, 24 freqs: {:.0} ms on {} threads", r.t_ms, r.threads);
        assert_eq!(r.points.len(), 24);
        assert!(r.points.iter().all(|p| p.z.re > 0.0 && p.ga_dbi.is_finite()));
    }
}
