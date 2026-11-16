//! Tools → Stack / bay array: copies of the antenna stacked one above another and/or
//! bayed side by side, every copy fed in phase, as through an ideal power splitter.

use crate::model::{Antenna, GroundKind, Wire};
use eframe::egui::{self, Color32, RichText};

/// The array's layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// copies one above another, and their spacing (m)
    pub n_stack: usize,
    pub d_stack: f64,
    /// copies side by side, and their spacing (m)
    pub n_bay: usize,
    pub d_bay: f64,
    /// azimuth of the main beam (degrees): copies are bayed across it
    pub beam_az: f64,
    /// wires kept once instead of copied (masts, booms): see [`share_wires`]
    pub shared: Vec<usize>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { n_stack: 2, d_stack: 0.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0, shared: Vec::new() }
    }
}

/// Spacing for least loss of gain to grating lobes, from the −3 dB beamwidth in the
/// plane the copies lie in (DL6WU): D = λ / (2·sin(BW/2)).
pub fn suggested_spacing(lambda: f64, beamwidth_deg: f64) -> f64 {
    lambda / (2.0 * (beamwidth_deg.to_radians() / 2.0).sin())
}

/// Spacing used where the pattern doesn't fall 3 dB in that plane (a dipole along its
/// broadside, a vertical round it): three-quarters of a wavelength.
pub const ROUND_SPACING_L: f64 = 0.75;

/// [`suggested_spacing`], or [`ROUND_SPACING_L`] wavelengths when there is no beamwidth.
pub fn spacing_for(lambda: f64, beamwidth_deg: Option<f64>) -> f64 {
    beamwidth_deg.map_or(ROUND_SPACING_L * lambda, |b| suggested_spacing(lambda, b))
}

/// The antenna's free-space beamwidths (degrees): (across the beam horizontally, which sets
/// the bay spacing; vertically, which sets the stack spacing), and its beam's azimuth.
pub fn free_space_beamwidths(ant: &Antenna) -> Result<(Option<f64>, Option<f64>, f64), String> {
    let mut a = ant.clone();
    a.ground.kind = GroundKind::Free;
    let c = crate::mom::calculate(&a)?;
    Ok((c.patterns.az_beamwidth(), c.patterns.el_beamwidth(), c.result.az_max))
}

/// Builds the array: the antenna's wires, plates, sources, loads and lines copied to each
/// place, centred on the original, every feed made a current source in phase.
pub fn build(ant: &Antenna, l: &Layout) -> Result<Antenna, String> {
    if l.n_stack == 0 || l.n_bay == 0 || l.n_stack * l.n_bay < 2 {
        return Err("Choose at least two antennas.".into());
    }
    if (l.n_stack > 1 && l.d_stack <= 0.0) || (l.n_bay > 1 && l.d_bay <= 0.0) {
        return Err("Give each spacing in use a length above 0.".into());
    }
    if ant.sources.is_empty() {
        return Err("The antenna has no source to feed the copies from.".into());
    }
    let az = l.beam_az.to_radians();
    let across = [-az.sin(), az.cos(), 0.0];
    let mut out = ant.clone();
    out.wires.clear();
    out.sources.clear();
    out.loads.clear();
    out.lines.clear();
    out.plates.clear();
    let shared: Vec<bool> = (0..ant.wires.len()).map(|i| l.shared.contains(&i)).collect();
    for (i, _) in shared.iter().enumerate().filter(|(_, s)| **s) {
        if ant.sources.iter().any(|s| s.wire == i) || ant.loads.iter().any(|d| d.wire == i) || ant.lines.iter().any(|t| t.a_wire == i || t.b_wire == i) {
            return Err(format!("Wire {} has a source, load or line on it, so it can't be shared: each copy needs its own.", i + 1));
        }
    }
    let mut offsets = Vec::new();
    for s in 0..l.n_stack {
        for b in 0..l.n_bay {
            let up = (s as f64 - (l.n_stack - 1) as f64 / 2.0) * l.d_stack;
            let side = (b as f64 - (l.n_bay - 1) as f64 / 2.0) * l.d_bay;
            let off = [across[0] * side, across[1] * side, up];
            offsets.push(off);
            let mv = |p: [f64; 3]| [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
            // where each of the antenna's wires went in this copy
            let mut map = vec![usize::MAX; ant.wires.len()];
            for (i, w) in ant.wires.iter().enumerate() {
                if shared[i] {
                    continue;
                }
                let mut w = w.clone();
                w.p1 = mv(w.p1);
                w.p2 = mv(w.p2);
                map[i] = out.wires.len();
                out.wires.push(w);
            }
            for src in &ant.sources {
                let mut src = src.clone();
                src.wire = map[src.wire];
                // in phase through an ideal splitter: each copy keeps its own feed current
                if !src.current {
                    src.current = true;
                    src.volts = 1.0;
                }
                out.sources.push(src);
            }
            for ld in &ant.loads {
                let mut ld = ld.clone();
                ld.wire = map[ld.wire];
                out.loads.push(ld);
            }
            for tl in &ant.lines {
                let mut tl = tl.clone();
                tl.a_wire = map[tl.a_wire];
                tl.b_wire = map[tl.b_wire];
                out.lines.push(tl);
            }
            for p in &ant.plates {
                let mut p = p.clone();
                p.corner = mv(p.corner);
                out.plates.push(p);
            }
        }
    }
    let ground = (ant.ground.kind != GroundKind::Free).then_some(-ant.ground.add_height);
    let kept: Vec<&Wire> = ant.wires.iter().zip(&shared).filter(|(_, s)| **s).map(|(w, _)| w).collect();
    let made = share_wires(&kept, &offsets, &out.wires, ground);
    out.wires.extend(made);
    if ant.ground.kind != GroundKind::Free {
        let lowest = out.wires.iter().flat_map(|w| [w.p1[2], w.p2[2]]).fold(f64::INFINITY, f64::min) + ant.ground.add_height;
        if lowest < 0.0 {
            return Err(format!("The lowest antenna would be {:.2} m below the ground: raise the antenna, or stack fewer or closer.", -lowest));
        }
    }
    out.name = format!("{} ({})", ant.name, describe(l));
    Ok(out)
}

/// Shared wires for the array. Each is moved only along its own line (a vertical mast follows
/// the stacking, not the baying), the pieces on one line are joined into one run (not below
/// the ground, when there is one), and the run is split wherever a copied wire ends on it, so
/// each copy's boom meets the mast at a wire end.
pub fn share_wires(kept: &[&Wire], offsets: &[[f64; 3]], copies: &[Wire], ground_z: Option<f64>) -> Vec<Wire> {
    const TOL: f64 = 1e-4;
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let at = |o: [f64; 3], u: [f64; 3], t: f64| [o[0] + u[0] * t, o[1] + u[1] * t, o[2] + u[2] * t];
    // one line: origin, unit direction, (from, to) intervals, the wire to copy settings from
    struct Run<'a> {
        o: [f64; 3],
        u: [f64; 3],
        spans: Vec<(f64, f64)>,
        like: &'a Wire,
    }
    let mut runs: Vec<Run> = Vec::new();
    for w in kept {
        let d = sub(w.p2, w.p1);
        let len = dot(d, d).sqrt();
        if len < TOL {
            continue;
        }
        let mut u = d.map(|c| c / len);
        // one direction per line
        let big = (0..3).max_by(|&a, &b| u[a].abs().total_cmp(&u[b].abs())).unwrap();
        if u[big] < 0.0 {
            u = u.map(|c| -c);
        }
        let on_line = |r: &Run, p: [f64; 3]| {
            let v = sub(p, r.o);
            let t = dot(v, r.u);
            let q = sub(v, r.u.map(|c| c * t));
            dot(q, q).sqrt() < TOL
        };
        let i = match runs.iter().position(|r| dot(r.u, u).abs() > 1.0 - 1e-9 && on_line(r, w.p1)) {
            Some(i) => i,
            None => {
                runs.push(Run { o: w.p1, u, spans: Vec::new(), like: w });
                runs.len() - 1
            }
        };
        let r = &mut runs[i];
        let (t1, t2) = (dot(sub(w.p1, r.o), r.u), dot(sub(w.p2, r.o), r.u));
        for off in offsets {
            let s = dot(*off, r.u);
            r.spans.push((t1.min(t2) + s, t1.max(t2) + s));
        }
    }
    // each run's joined spans, not below the ground
    let mut joined: Vec<Vec<(f64, f64)>> = Vec::new();
    for r in &runs {
        let (lo_t, hi_t) = match ground_z {
            Some(g) if r.u[2].abs() > 1e-9 => {
                let tg = (g - r.o[2]) / r.u[2];
                if r.u[2] > 0.0 { (tg, f64::INFINITY) } else { (f64::NEG_INFINITY, tg) }
            }
            _ => (f64::NEG_INFINITY, f64::INFINITY),
        };
        let mut spans: Vec<(f64, f64)> = r.spans.iter().map(|&(a, b)| (a.max(lo_t), b.min(hi_t))).filter(|(a, b)| b - a > TOL).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(m) if a <= m.1 + TOL => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        joined.push(merged);
    }
    // the ends that may meet a run: copied wires', and other shared runs'
    let mut ends: Vec<[f64; 3]> = copies.iter().flat_map(|w| [w.p1, w.p2]).collect();
    for (r, m) in runs.iter().zip(&joined) {
        for &(a, b) in m {
            ends.extend([at(r.o, r.u, a), at(r.o, r.u, b)]);
        }
    }
    let mut out = Vec::new();
    for (r, merged) in runs.iter().zip(joined) {
        let like_len = r.like.length();
        for (a, b) in merged {
            let mut cuts = vec![a, b];
            for &p in &ends {
                let v = sub(p, r.o);
                let t = dot(v, r.u);
                let q = sub(v, r.u.map(|c| c * t));
                if dot(q, q).sqrt() < TOL && t > a + TOL && t < b - TOL {
                    cuts.push(t);
                }
            }
            cuts.sort_by(|x, y| x.total_cmp(y));
            cuts.dedup_by(|x, y| (*x - *y).abs() < TOL);
            for pair in cuts.windows(2) {
                let mut w = r.like.clone();
                w.p1 = at(r.o, r.u, pair[0]);
                w.p2 = at(r.o, r.u, pair[1]);
                if r.like.seg > 0 {
                    w.seg = ((r.like.seg as f64 * (pair[1] - pair[0]) / like_len).round() as i32).max(1);
                }
                out.push(w);
            }
        }
    }
    out
}

/// "2 stacked × 2 bayed", "4 stacked", "2 bayed".
pub fn describe(l: &Layout) -> String {
    match (l.n_stack > 1, l.n_bay > 1) {
        (true, true) => format!("{} stacked × {} bayed", l.n_stack, l.n_bay),
        (true, false) => format!("{} stacked", l.n_stack),
        _ => format!("{} bayed", l.n_bay),
    }
}

// ------------------------------------------------------------------ the window

pub struct StackState {
    pub open: bool,
    pub layout: Layout,
    /// free-space beamwidths found for the suggestion: (horizontal, vertical, beam azimuth)
    hint: Option<(Option<f64>, Option<f64>, f64)>,
    error: Option<String>,
}

impl Default for StackState {
    fn default() -> Self {
        StackState { open: false, layout: Layout::default(), hint: None, error: None }
    }
}

impl StackState {
    /// Fills the spacings and beam direction from the antenna's free-space beamwidths.
    pub fn suggest(&mut self, ant: &Antenna) {
        match free_space_beamwidths(ant) {
            Ok(h) => {
                let lambda = ant.lambda();
                self.layout.d_stack = (spacing_for(lambda, h.1) * 1000.0).round() / 1000.0;
                self.layout.d_bay = (spacing_for(lambda, h.0) * 1000.0).round() / 1000.0;
                self.layout.beam_az = h.2;
                self.hint = Some(h);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// A new antenna was loaded: suggestions from the old one no longer apply.
    pub fn for_antenna(&mut self) {
        self.hint = None;
        self.error = None;
        self.layout.d_stack = 0.0;
        self.layout.d_bay = 0.0;
        self.layout.shared.clear();
    }
}

/// The window; returns the array to put in place of the antenna when asked.
pub fn window(ctx: &egui::Context, st: &mut StackState, ant: &Antenna) -> Option<Antenna> {
    if !st.open {
        return None;
    }
    // spacings start from the suggestion
    if st.hint.is_none() && st.error.is_none() && st.layout.d_stack == 0.0 && st.layout.d_bay == 0.0 {
        st.suggest(ant);
    }
    let mut made = None;
    let mut open = st.open;
    egui::Window::new("Stack / bay array").open(&mut open).default_width(520.0).show(ctx, |ui| {
        ui.label("Copies of this antenna one above another (stacked) and/or side by side across the beam (bayed), all fed in phase.");
        ui.add_space(4.0);
        let lambda = ant.lambda();
        egui::Grid::new("stack_in").spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Stacked");
            ui.add(egui::DragValue::new(&mut st.layout.n_stack).range(1..=8).suffix(" high"));
            ui.add_enabled(st.layout.n_stack > 1, egui::DragValue::new(&mut st.layout.d_stack).speed(0.01).range(0.0..=1000.0).suffix(" m apart"));
            ui.label(RichText::new(format!("{:.2} λ", st.layout.d_stack / lambda)).color(Color32::GRAY));
            ui.end_row();
            ui.label("Bayed");
            ui.add(egui::DragValue::new(&mut st.layout.n_bay).range(1..=8).suffix(" wide"));
            ui.add_enabled(st.layout.n_bay > 1, egui::DragValue::new(&mut st.layout.d_bay).speed(0.01).range(0.0..=1000.0).suffix(" m apart"));
            ui.label(RichText::new(format!("{:.2} λ", st.layout.d_bay / lambda)).color(Color32::GRAY));
            ui.end_row();
            ui.label("Beam towards");
            ui.add(egui::DragValue::new(&mut st.layout.beam_az).speed(1.0).range(0.0..=359.9).suffix("° azimuth"))
                .on_hover_text("Bayed copies sit side by side across this direction.");
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Suggest spacings").on_hover_text("From the antenna's −3 dB beamwidths in free space: D = λ / (2·sin(BW/2)) (DL6WU), the usual compromise between gain and side lobes.").clicked() {
                st.suggest(ant);
            }
            if let Some((h, v, _)) = st.hint {
                let t = |b: Option<f64>| b.map_or("—".to_string(), |b| format!("{b:.1}°"));
                ui.label(RichText::new(format!("free-space beamwidth: vertical {}, horizontal {}", t(v), t(h))).color(Color32::GRAY))
                    .on_hover_text(format!("— : the pattern is round in that plane, so {ROUND_SPACING_L} λ is suggested."));
            }
        });
        egui::CollapsingHeader::new(format!("Shared wires ({})", st.layout.shared.len())).id_salt("stack_shared").show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Ticked wires (a mast, a cross boom) are kept once instead of copied. Each is moved only along its own line, \
                     joined into one run and split where a copy's wire ends on it: stacked Yagis share one mast.",
                )
                .small()
                .color(Color32::GRAY),
            );
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for (i, w) in ant.wires.iter().enumerate() {
                    let d = [w.p2[0] - w.p1[0], w.p2[1] - w.p1[1], w.p2[2] - w.p1[2]];
                    let len = w.length();
                    let dir = if len > 0.0 && d[2].abs() / len > 0.99 {
                        "vertical"
                    } else if len > 0.0 && d[2].abs() / len < 0.01 {
                        "horizontal"
                    } else {
                        "sloping"
                    };
                    let mut on = st.layout.shared.contains(&i);
                    if ui.checkbox(&mut on, format!("Wire {}: {dir}, {len:.2} m", i + 1)).changed() {
                        st.layout.shared.retain(|&k| k != i);
                        if on {
                            st.layout.shared.push(i);
                        }
                    }
                }
            });
        });
        if let Some(e) = &st.error {
            ui.colored_label(Color32::from_rgb(190, 60, 30), e);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Each copy's feed becomes a current source of the same amplitude and phase: an ideal splitter and equal phasing lines. \
                 The array replaces the antenna (Undo brings it back); press Calculate to compare.",
            )
            .small()
            .color(Color32::GRAY),
        );
        let n = st.layout.n_stack * st.layout.n_bay;
        if ui.add_enabled(n >= 2, egui::Button::new(format!("Make the {} array ({n} antennas)", describe(&st.layout)))).clicked() {
            match build(ant, &st.layout) {
                Ok(a) => {
                    made = Some(a);
                    st.error = None;
                }
                Err(e) => st.error = Some(e),
            }
        }
    });
    st.open = open && made.is_none();
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dl6wu_spacing() {
        // 60° beamwidth: D = λ
        assert!((suggested_spacing(2.0, 60.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn four_bay_yagi_in_free_space() {
        let mut y = Antenna::example(2);
        y.ground.kind = GroundKind::Free;
        let (h, v, az) = free_space_beamwidths(&y).unwrap();
        let (h, v) = (h.unwrap(), v.unwrap());
        let lambda = y.lambda();
        let l = Layout { n_stack: 2, d_stack: suggested_spacing(lambda, v), n_bay: 2, d_bay: suggested_spacing(lambda, h), beam_az: az, shared: vec![] };
        let a = build(&y, &l).unwrap();
        assert_eq!(a.wires.len(), 4 * y.wires.len());
        assert_eq!(a.sources.len(), 4);
        assert!(a.sources.iter().all(|s| s.current && s.volts == 1.0));
        // the copies sit about the original: the centre of all wires is unchanged
        let centre = |a: &Antenna| {
            let n = a.wires.len() as f64 * 2.0;
            [0, 1, 2].map(|c| a.wires.iter().map(|w| w.p1[c] + w.p2[c]).sum::<f64>() / n)
        };
        let (c0, c1) = (centre(&y), centre(&a));
        assert!((0..3).all(|c| (c0[c] - c1[c]).abs() < 1e-9), "{c0:?} {c1:?}");
        // four Yagis at the DL6WU spacing: close to 6 dB more gain
        let g1 = crate::mom::calculate(&y).unwrap().result.ga_dbi;
        let g4 = crate::mom::calculate(&a).unwrap().result.ga_dbi;
        assert!(g4 - g1 > 5.0 && g4 - g1 < 6.8, "{g1:.2} → {g4:.2}");
    }

    #[test]
    fn stacking_into_the_ground_is_refused() {
        let y = Antenna::example(2); // 15 m up
        let l = Layout { n_stack: 4, d_stack: 12.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0, shared: vec![] };
        assert!(build(&y, &l).unwrap_err().contains("below the ground"));
    }

    /// A dipole on a stub boom out from a mast: wire 0 the dipole (fed), 1 the stub, 2 the mast.
    fn on_a_mast() -> Antenna {
        let mut a = Antenna::example(0);
        let r = a.wires[0].r;
        let h = 10.0;
        let half = a.lambda() / 4.0 * 0.95;
        a.wires = vec![Wire::new([1.0, -half, h], [1.0, half, h], r), Wire::new([0.0, 0.0, h], [1.0, 0.0, h], r), Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, h + 0.5], r)];
        a.sources.truncate(1);
        a.sources[0].wire = 0;
        a.sources[0].pos = 0.5;
        a.loads.clear();
        a.lines.clear();
        a.ground.kind = GroundKind::Real;
        a.ground.add_height = 0.0;
        a
    }

    #[test]
    fn stacked_on_one_mast() {
        let a = on_a_mast();
        let l = Layout { n_stack: 2, d_stack: 3.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0, shared: vec![2] };
        let s = build(&a, &l).unwrap();
        // two dipoles, two stubs, and the mast from the ground to 12 m split at both stubs
        assert_eq!(s.wires.len(), 7, "{:?}", s.wires.iter().map(|w| (w.p1, w.p2)).collect::<Vec<_>>());
        let mut mast: Vec<(f64, f64)> = s.wires[4..].iter().map(|w| (w.p1[2], w.p2[2])).collect();
        mast.sort_by(|a, b| a.0.total_cmp(&b.0));
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        assert!(near(mast[0], (0.0, 8.5)) && near(mast[1], (8.5, 11.5)) && near(mast[2], (11.5, 12.0)), "{mast:?}");
        assert!(s.wires[4..].iter().all(|w| w.p1[0] == 0.0 && w.p2[0] == 0.0));
        assert_eq!(s.sources.iter().map(|x| x.wire).collect::<Vec<_>>(), vec![0, 2]);
        crate::mom::calculate(&s).unwrap();
    }

    #[test]
    fn bayed_beside_one_mast() {
        // copies go sideways: the vertical mast stays where it is, once
        let a = on_a_mast();
        let l = Layout { n_stack: 1, d_stack: 0.0, n_bay: 2, d_bay: 4.0, beam_az: 0.0, shared: vec![2] };
        let s = build(&a, &l).unwrap();
        assert_eq!(s.wires.len(), 5);
        let m = &s.wires[4];
        assert_eq!((m.p1, m.p2), (a.wires[2].p1, a.wires[2].p2));
    }

    #[test]
    fn fed_wires_are_not_shared() {
        let a = on_a_mast();
        let l = Layout { n_stack: 2, d_stack: 3.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0, shared: vec![0] };
        assert!(build(&a, &l).unwrap_err().contains("can't be shared"));
    }
}
