//! Tools → Stack / bay array: copies of the antenna stacked one above another and/or
//! bayed side by side, every copy fed in phase, as through an ideal power splitter.

use crate::model::{Antenna, GroundKind};
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
}

impl Default for Layout {
    fn default() -> Self {
        Layout { n_stack: 2, d_stack: 0.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0 }
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
    let nw = ant.wires.len();
    for s in 0..l.n_stack {
        for b in 0..l.n_bay {
            let up = (s as f64 - (l.n_stack - 1) as f64 / 2.0) * l.d_stack;
            let side = (b as f64 - (l.n_bay - 1) as f64 / 2.0) * l.d_bay;
            let off = [across[0] * side, across[1] * side, up];
            let mv = |p: [f64; 3]| [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
            let base = out.wires.len();
            for w in &ant.wires {
                let mut w = w.clone();
                w.p1 = mv(w.p1);
                w.p2 = mv(w.p2);
                out.wires.push(w);
            }
            for src in &ant.sources {
                let mut src = src.clone();
                src.wire += base;
                // in phase through an ideal splitter: each copy keeps its own feed current
                if !src.current {
                    src.current = true;
                    src.volts = 1.0;
                }
                out.sources.push(src);
            }
            for ld in &ant.loads {
                let mut ld = ld.clone();
                ld.wire += base;
                out.loads.push(ld);
            }
            for tl in &ant.lines {
                let mut tl = tl.clone();
                tl.a_wire += base;
                tl.b_wire += base;
                out.lines.push(tl);
            }
            for p in &ant.plates {
                let mut p = p.clone();
                p.corner = mv(p.corner);
                out.plates.push(p);
            }
        }
    }
    debug_assert_eq!(out.wires.len(), nw * l.n_stack * l.n_bay);
    if ant.ground.kind != GroundKind::Free {
        let lowest = out.wires.iter().flat_map(|w| [w.p1[2], w.p2[2]]).fold(f64::INFINITY, f64::min) + ant.ground.add_height;
        if lowest < 0.0 {
            return Err(format!("The lowest antenna would be {:.2} m below the ground: raise the antenna, or stack fewer or closer.", -lowest));
        }
    }
    out.name = format!("{} ({})", ant.name, describe(l));
    Ok(out)
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
        let l = Layout { n_stack: 2, d_stack: suggested_spacing(lambda, v), n_bay: 2, d_bay: suggested_spacing(lambda, h), beam_az: az };
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
        let l = Layout { n_stack: 4, d_stack: 12.0, n_bay: 1, d_bay: 0.0, beam_az: 0.0 };
        assert!(build(&y, &l).unwrap_err().contains("below the ground"));
    }
}

