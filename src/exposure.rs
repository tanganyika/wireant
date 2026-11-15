//! Tools → RF exposure: the near field on a plane around the antenna, against the
//! reference levels of an exposure standard.

use crate::mom::FullResult;
use crate::model::{Antenna, GroundKind};
use crate::nearfield::{self, Currents, Standard};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::sync::mpsc::{channel, Receiver};

type V3 = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// level, at a height above the ground (or z in free space)
    Horizontal,
    /// upright, through the antenna's middle, along x (seen from the side)
    AlongX,
    /// upright, through the antenna's middle, along y
    AlongY,
}

impl Plane {
    fn label(&self) -> &'static str {
        match self {
            Plane::Horizontal => "Level, at a height",
            Plane::AlongX => "Upright, along x",
            Plane::AlongY => "Upright, along y",
        }
    }
}

/// Transmission modes and the share of the PEP they average to while keyed.
pub const MODES: [(&str, f64); 6] = [
    ("SSB voice (20 %)", 0.2),
    ("SSB voice, processed (50 %)", 0.5),
    ("CW (40 %)", 0.4),
    ("FT8 and other data (100 %)", 1.0),
    ("FM, AM carrier, RTTY (100 %)", 1.0),
    ("Constant carrier (100 %)", 1.0),
];

/// The plane's settings, which fix what is calculated.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub plane: Plane,
    /// height of a level plane (m)
    pub height: f64,
    /// how far the plane reaches past the antenna on every side (m)
    pub margin: f64,
    /// points along the longer side
    pub n: usize,
}

/// A calculated plane: points on a grid, their fields scaled to 1 W input.
pub struct Map {
    pub setup: Setup,
    pub freq_mhz: f64,
    /// plane coordinates of the grid's corners: (u0, v0) to (u1, v1)
    pub u: (f64, f64),
    pub v: (f64, f64),
    pub nu: usize,
    pub nv: usize,
    /// per point (row-major, v then u): RMS E (V/m) and H (A/m) at 1 W into the antenna
    pub eh: Vec<(f64, f64)>,
    /// per point: distance to the nearest wire (m)
    pub dist: Vec<f64>,
    /// the wires, as (a, b) in plane coordinates
    pub wires: Vec<([f64; 2], [f64; 2])>,
    pub ground: bool,
}

pub struct ExposureState {
    pub open: bool,
    pub pep_w: f64,
    pub mode: usize,
    /// share of the averaging time spent transmitting (0..1)
    pub on_air: f64,
    pub standard: Standard,
    pub setup: Setup,
    pub map: Option<Map>,
    job: Option<Receiver<Result<Map, String>>>,
    error: Option<String>,
    /// test hook: work out the map as soon as there is a calculation (screenshots)
    pub auto: bool,
}

impl Default for ExposureState {
    fn default() -> Self {
        ExposureState {
            open: false,
            pep_w: 100.0,
            mode: 0,
            on_air: 0.5,
            standard: Standard::Icnirp1998,
            setup: Setup { plane: Plane::Horizontal, height: 2.0, margin: 10.0, n: 101 },
            map: None,
            job: None,
            error: None,
            auto: false,
        }
    }
}

impl ExposureState {
    /// Average power into the antenna: PEP × mode × time on air.
    pub fn avg_w(&self) -> f64 {
        self.pep_w * MODES[self.mode].1 * self.on_air
    }
}

/// Fields on `setup`'s plane for the currents in `cur`, scaled to 1 W into the antenna.
pub fn compute(cur: &Currents, ant: &Antenna, freq_mhz: f64, setup: &Setup) -> Result<Map, String> {
    if cur.filaments.is_empty() || cur.p_in <= 0.0 {
        return Err("This calculation has no currents to work from: press Calculate first.".into());
    }
    let ground = ant.ground.kind != GroundKind::Free;
    let (lo, hi) = bounds(&cur.filaments);
    let m = setup.margin.max(0.5);
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    // plane coordinates (u, v) and the 3D point they stand for
    let (u, v, to3): ((f64, f64), (f64, f64), Box<dyn Fn(f64, f64) -> V3 + Sync>) = match setup.plane {
        Plane::Horizontal => {
            let h = setup.height;
            ((lo[0] - m, hi[0] + m), (lo[1] - m, hi[1] + m), Box::new(move |u, v| [u, v, h]))
        }
        Plane::AlongX => {
            let z0 = if ground { 0.0 } else { lo[2] - m };
            let y = centre[1];
            ((lo[0] - m, hi[0] + m), (z0, hi[2] + m), Box::new(move |u, v| [u, y, v]))
        }
        Plane::AlongY => {
            let z0 = if ground { 0.0 } else { lo[2] - m };
            let x = centre[0];
            ((lo[1] - m, hi[1] + m), (z0, hi[2] + m), Box::new(move |u, v| [x, u, v]))
        }
    };
    let n = setup.n.clamp(11, 201);
    let (du, dv) = (u.1 - u.0, v.1 - v.0);
    let step = du.max(dv) / (n - 1) as f64;
    let nu = (du / step).round() as usize + 1;
    let nv = (dv / step).round() as usize + 1;
    let mut pts = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for i in 0..nu {
            let p = to3(u.0 + i as f64 * step, v.0 + j as f64 * step);
            // just above the ground, not on it
            pts.push([p[0], p[1], if ground { p[2].max(0.01) } else { p[2] }]);
        }
    }
    let f = nearfield::fields(cur, ant, freq_mhz, &pts);
    let s = (1.0 / cur.p_in).sqrt();
    let eh = f.iter().map(|(e, h)| {
        let (er, hr) = nearfield::rms(e, h);
        (er * s, hr * s)
    }).collect();
    let dist = pts.iter().map(|p| nearest_wire(&cur.filaments, *p)).collect();
    let flat = |p: V3| -> [f64; 2] {
        match setup.plane {
            Plane::Horizontal => [p[0], p[1]],
            Plane::AlongX => [p[0], p[2]],
            Plane::AlongY => [p[1], p[2]],
        }
    };
    let wires = cur.filaments.iter().map(|f| (flat(f.a), flat(f.b))).collect();
    Ok(Map { setup: setup.clone(), freq_mhz, u: (u.0, u.0 + (nu - 1) as f64 * step), v: (v.0, v.0 + (nv - 1) as f64 * step), nu, nv, eh, dist, wires, ground })
}

/// What a map says at `avg_w` average watts against `std`.
pub struct Summary {
    /// largest share of the limit on the plane at least [`TOUCH_M`] from a wire, and where
    /// (index; the first point when there is none)
    pub max_ratio: f64,
    pub at: usize,
    /// farthest distance from a wire at which the limit is reached, if anywhere
    pub over_to: Option<f64>,
    pub limits: (f64, f64),
}

/// Closer than this to a wire counts as touching it: such points still colour the map
/// and count for [`Summary::over_to`], but not for the strongest point.
pub const TOUCH_M: f64 = 0.5;

pub fn summarise(map: &Map, avg_w: f64, std: Standard) -> Option<Summary> {
    let lim = std.limits(map.freq_mhz)?;
    let s = avg_w.max(0.0).sqrt();
    let mut best = (0.0f64, 0usize);
    let mut over: Option<f64> = None;
    for (k, &(e, h)) in map.eh.iter().enumerate() {
        let r = nearfield::ratio(e * s, h * s, lim);
        if r > best.0 && map.dist[k] >= TOUCH_M {
            best = (r, k);
        }
        if r >= 1.0 {
            over = Some(over.unwrap_or(0.0).max(map.dist[k]));
        }
    }
    Some(Summary { max_ratio: best.0, at: best.1, over_to: over, limits: lim })
}

fn bounds(fils: &[nearfield::Filament]) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for f in fils {
        for p in [f.a, f.b] {
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
    }
    (lo, hi)
}

fn nearest_wire(fils: &[nearfield::Filament], p: V3) -> f64 {
    fils.iter().map(|f| {
        let ab = [f.b[0] - f.a[0], f.b[1] - f.a[1], f.b[2] - f.a[2]];
        let ap = [p[0] - f.a[0], p[1] - f.a[1], p[2] - f.a[2]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        let t = if l2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = [ap[0] - t * ab[0], ap[1] - t * ab[1], ap[2] - t * ab[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }).fold(f64::INFINITY, f64::min)
}

/// Colour for a share of the limit: greens well below, yellow to orange towards it, red over.
fn ratio_colour(r: f64) -> Color32 {
    let [a, b, c] = ratio_rgb(r);
    Color32::from_rgb(a, b, c)
}

/// [`ratio_colour`] as RGB, for the PDF report.
pub fn ratio_rgb(r: f64) -> [u8; 3] {
    if r >= 1.0 {
        return [215, 40, 40];
    }
    // log scale from 1 % to 100 %
    let t = ((r.max(1e-2).log10() + 2.0) / 2.0) as f32;
    let stops = [(0.0, [225, 240, 225]), (0.5, [150, 205, 120]), (0.8, [240, 210, 80]), (1.0, [240, 140, 40])];
    for w in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f) as u8;
            return [m(c0[0], c1[0]), m(c0[1], c1[1]), m(c0[2], c1[2])];
        }
    }
    [240, 140, 40]
}

/// The legend's steps: (share of the limit, label).
pub const LEGEND: [(f64, &str); 5] = [(0.01, "≤1 %"), (0.1, "10 %"), (0.3, "30 %"), (0.7, "70 %"), (1.0, "≥100 % of the limit")];

/// How the fields are found and what the limits are, for under the map.
pub fn method_note(ground: GroundKind) -> &'static str {
    match ground {
        GroundKind::Real => "Fields from the calculated currents, with the ground's reflection approximated (reflection coefficients at the specular angle): within about 1 dB of NEC-2's Sommerfeld near fields a few metres up, less exact right at the ground. Limits are time-averaged reference levels for the general public; check them against your regulator's own figures, and remember feed-line loss lowers the power that reaches the antenna.",
        _ => "Fields from the calculated currents (they agree with NEC-2's own near fields to within 0.1 dB). Limits are time-averaged reference levels for the general public; check them against your regulator's own figures, and remember feed-line loss lowers the power that reaches the antenna.",
    }
}

/// The planes the PDF report shows: level at the window's height, and upright along
/// the window's upright direction (x unless y was chosen), both reaching as far.
pub fn report_planes(st: &ExposureState) -> [Setup; 2] {
    let up = if st.setup.plane == Plane::AlongY { Plane::AlongY } else { Plane::AlongX };
    [Setup { plane: Plane::Horizontal, ..st.setup.clone() }, Setup { plane: up, ..st.setup.clone() }]
}

impl Plane {
    /// A title for the plane, e.g. "Level plane 2.0 m up".
    pub fn title(&self, setup: &Setup, free_space: bool) -> String {
        match self {
            Plane::Horizontal if free_space => format!("Level plane at z = {:.1} m", setup.height),
            Plane::Horizontal => format!("Level plane {:.1} m above the ground", setup.height),
            Plane::AlongX => "Upright plane through the antenna, along x".into(),
            Plane::AlongY => "Upright plane through the antenna, along y".into(),
        }
    }

    /// The plane's axis names (u, v).
    pub fn axes(&self) -> (&'static str, &'static str) {
        match self {
            Plane::Horizontal => ("x", "y"),
            Plane::AlongX => ("x", "z"),
            Plane::AlongY => ("y", "z"),
        }
    }
}

pub fn pct(r: f64) -> String {
    if r >= 0.1 { format!("{:.0} %", r * 100.0) } else { format!("{:.1} %", r * 100.0) }
}

/// The RF exposure window.
pub fn window(ctx: &egui::Context, st: &mut ExposureState, ant: &Antenna, calc: Option<&FullResult>) {
    if !st.open {
        return;
    }
    // a finished calculation
    if let Some(rx) = &st.job {
        match rx.try_recv() {
            Ok(r) => {
                st.job = None;
                match r {
                    Ok(m) => {
                        st.map = Some(m);
                        st.error = None;
                    }
                    Err(e) => st.error = Some(e),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
            Err(_) => st.job = None,
        }
    }
    let mut open = st.open;
    egui::Window::new("RF exposure").open(&mut open).default_width(760.0).default_height(720.0).show(ctx, |ui| {
        let Some(c) = calc else {
            ui.label(RichText::new("Press Calculate first: the fields come from the calculated currents.").italics());
            return;
        };
        let f = c.result.freq_mhz;
        if st.auto && st.map.is_none() {
            st.auto = false;
            match compute(&c.near, ant, f, &st.setup) {
                Ok(m) => st.map = Some(m),
                Err(e) => st.error = Some(e),
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("PEP into the antenna");
            ui.add(egui::DragValue::new(&mut st.pep_w).speed(1.0).range(0.0..=100_000.0).suffix(" W"));
            ui.label("Mode");
            egui::ComboBox::from_id_salt("exp_mode").selected_text(MODES[st.mode].0).show_ui(ui, |ui| {
                for (i, (name, _)) in MODES.iter().enumerate() {
                    ui.selectable_value(&mut st.mode, i, *name);
                }
            });
            ui.label("On air");
            let mut on = st.on_air * 100.0;
            if ui.add(egui::DragValue::new(&mut on).speed(1.0).range(1.0..=100.0).suffix(" %")).on_hover_text("Share of the averaging time (6 minutes for ICNIRP 1998 and the FCC's controlled limits, 30 minutes for the FCC's general population and ICNIRP 2020) spent transmitting.").changed() {
                st.on_air = on / 100.0;
            }
            ui.label(RichText::new(format!("= {:.1} W average", st.avg_w())).strong());
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Standard");
            egui::ComboBox::from_id_salt("exp_std").selected_text(st.standard.label()).show_ui(ui, |ui| {
                for s in Standard::ALL {
                    ui.selectable_value(&mut st.standard, s, s.label());
                }
            });
            match st.standard.limits(f) {
                Some((e, h)) => ui.label(format!("limits at {f:.3} MHz: {e:.1} V/m, {h:.4} A/m")),
                None => ui.label(RichText::new(format!("{} doesn't cover {f:.3} MHz", st.standard.label())).color(Color32::from_rgb(190, 60, 30))),
            };
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Plane");
            for p in [Plane::Horizontal, Plane::AlongX, Plane::AlongY] {
                ui.selectable_value(&mut st.setup.plane, p, p.label());
            }
            if st.setup.plane == Plane::Horizontal {
                ui.label(if ant.ground.kind == GroundKind::Free { "z" } else { "height" });
                ui.add(egui::DragValue::new(&mut st.setup.height).speed(0.1).range(-1000.0..=1000.0).suffix(" m"));
            }
            ui.label("reaching");
            ui.add(egui::DragValue::new(&mut st.setup.margin).speed(0.5).range(0.5..=500.0).suffix(" m"))
                .on_hover_text("How far the plane reaches past the antenna on every side.");
            let busy = st.job.is_some();
            if ui.add_enabled(!busy, egui::Button::new(if busy { "Working…" } else { "▶ Calculate fields" })).clicked() {
                let (tx, rx) = channel();
                let (cur, a, setup) = (c.near.clone(), ant.clone(), st.setup.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(compute(&cur, &a, f, &setup));
                });
                st.job = Some(rx);
            }
        });
        if let Some(e) = &st.error {
            ui.colored_label(Color32::from_rgb(190, 60, 30), e);
        }
        let Some(map) = &st.map else {
            ui.add_space(20.0);
            ui.label(RichText::new("Choose a plane and press Calculate fields.").italics().color(Color32::GRAY));
            return;
        };
        let stale = (map.freq_mhz - f).abs() > 1e-9 || map.setup != st.setup;
        if stale {
            ui.colored_label(Color32::from_rgb(170, 110, 0), "The settings or the calculation have changed since: press Calculate fields to update the map.");
        }
        let avg = st.avg_w();
        let Some(sum) = summarise(map, avg, st.standard) else {
            ui.label(format!("{} has no limits at {:.3} MHz.", st.standard.label(), map.freq_mhz));
            return;
        };
        let (e1, h1) = map.eh[sum.at];
        let s = avg.sqrt();
        let verdict = match sum.over_to {
            Some(d) => RichText::new(format!("Over the limit on this plane: up to {d:.1} m from the antenna."))
            .color(Color32::from_rgb(190, 30, 30))
            .strong(),
            None => RichText::new(format!("Within the limit everywhere on this plane: at most {} of it.", pct(sum.max_ratio))).color(Color32::from_rgb(30, 120, 40)).strong(),
        };
        ui.label(verdict);
        if map.dist[sum.at] >= TOUCH_M {
            ui.label(format!(
                "Strongest point {TOUCH_M} m or more from the wires: {} of the limit (E {:.1} V/m, H {:.3} A/m), {:.1} m from the nearest wire.",
                pct(sum.max_ratio),
                e1 * s,
                h1 * s,
                map.dist[sum.at]
            ));
        }
        if c.near.plates_left_out {
            ui.colored_label(Color32::from_rgb(170, 110, 0), "The plates' currents aren't included in these fields.");
        }
        draw_map(ui, map, avg, st.standard, sum.limits);
        ui.label(RichText::new(method_note(ant.ground.kind)).small().color(Color32::GRAY));
    });
    st.open = open;
}

fn draw_map(ui: &mut egui::Ui, map: &Map, avg_w: f64, std: Standard, lim: (f64, f64)) {
    let avail = ui.available_size();
    let size = Vec2::new(avail.x, (avail.y - 40.0).max(320.0));
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, Color32::from_rgb(250, 250, 252));
    let (du, dv) = (map.u.1 - map.u.0, map.v.1 - map.v.0);
    let k = ((rect.width() - 70.0) as f64 / du).min((rect.height() - 56.0) as f64 / dv);
    let w = (du * k) as f32;
    let h = (dv * k) as f32;
    let origin = Pos2::new(rect.left() + 50.0 + (rect.width() - 70.0 - w) / 2.0, rect.top() + 10.0 + h);
    let to = |u: f64, v: f64| Pos2::new(origin.x + ((u - map.u.0) * k) as f32, origin.y - ((v - map.v.0) * k) as f32);
    let step_u = du / (map.nu - 1).max(1) as f64;
    let step_v = dv / (map.nv - 1).max(1) as f64;
    let s = avg_w.max(0.0).sqrt();
    for j in 0..map.nv {
        for i in 0..map.nu {
            let (e, hh) = map.eh[j * map.nu + i];
            let r = nearfield::ratio(e * s, hh * s, lim);
            let (u, v) = (map.u.0 + i as f64 * step_u, map.v.0 + j as f64 * step_v);
            let a = to(u - step_u / 2.0, v + step_v / 2.0);
            let b = to(u + step_u / 2.0, v - step_v / 2.0);
            painter.rect_filled(Rect::from_two_pos(a, b).intersect(Rect::from_two_pos(to(map.u.0, map.v.1), to(map.u.1, map.v.0))), 0.0, ratio_colour(r));
        }
    }
    painter.rect_stroke(Rect::from_two_pos(to(map.u.0, map.v.1), to(map.u.1, map.v.0)), 0.0, Stroke::new(1.0, crate::plots::GRID), egui::StrokeKind::Outside);
    if map.ground && map.setup.plane != Plane::Horizontal {
        painter.line_segment([to(map.u.0, 0.0), to(map.u.1, 0.0)], Stroke::new(2.0, Color32::from_rgb(150, 120, 70)));
    }
    for (a, b) in &map.wires {
        painter.line_segment([to(a[0], a[1]), to(b[0], b[1])], Stroke::new(2.0, Color32::from_rgb(30, 50, 110)));
    }
    // axis labels: the ends of each side
    let (un, vn) = map.setup.plane.axes();
    let ink = crate::plots::INK;
    let f = FontId::proportional(11.0);
    painter.text(to(map.u.0, map.v.0) + Vec2::new(0.0, 4.0), Align2::LEFT_TOP, format!("{un} {:.1} m", map.u.0), f.clone(), ink);
    painter.text(to(map.u.1, map.v.0) + Vec2::new(0.0, 4.0), Align2::RIGHT_TOP, format!("{:.1} m", map.u.1), f.clone(), ink);
    painter.text(to(map.u.0, map.v.0) + Vec2::new(-4.0, 0.0), Align2::RIGHT_BOTTOM, format!("{vn} {:.1}", map.v.0), f.clone(), ink);
    painter.text(to(map.u.0, map.v.1) + Vec2::new(-4.0, 0.0), Align2::RIGHT_TOP, format!("{:.1} m", map.v.1), f.clone(), ink);
    // legend
    let ly = rect.bottom() - 10.0;
    let mut lx = rect.left() + 8.0;
    for (r, t) in LEGEND {
        painter.rect_filled(Rect::from_min_size(Pos2::new(lx, ly - 5.0), Vec2::new(14.0, 10.0)), 2.0, ratio_colour(r));
        let g = painter.layout_no_wrap(t.to_string(), f.clone(), ink);
        let wdt = g.size().x;
        painter.galley(Pos2::new(lx + 18.0, ly - g.size().y / 2.0), g, ink);
        lx += 26.0 + wdt;
    }
    // hover readout
    if let Some(m) = resp.hover_pos() {
        let u = map.u.0 + (m.x - origin.x) as f64 / k;
        let v = map.v.0 + (origin.y - m.y) as f64 / k;
        if u >= map.u.0 && u <= map.u.1 && v >= map.v.0 && v <= map.v.1 {
            let i = ((u - map.u.0) / step_u).round() as usize;
            let j = ((v - map.v.0) / step_v).round() as usize;
            let idx = j.min(map.nv - 1) * map.nu + i.min(map.nu - 1);
            let (e, hh) = map.eh[idx];
            let r = nearfield::ratio(e * s, hh * s, lim);
            let txt = format!(
                "{un} {u:.1} m, {vn} {v:.1} m  ({:.1} m from the antenna)\nE {:.2} V/m   H {:.4} A/m\n{} of the {} limit",
                map.dist[idx],
                e * s,
                hh * s,
                pct(r),
                std.label()
            );
            let g = painter.layout_no_wrap(txt, FontId::proportional(12.0), ink);
            let at = Pos2::new(rect.right() - g.size().x - 14.0, rect.top() + 8.0);
            painter.rect_filled(Rect::from_min_size(at, g.size()).expand(5.0), 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
            painter.galley(at, g, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dipole_plane_and_verdict() {
        let a = Antenna::example(0);
        let c = crate::mom::calculate(&a).unwrap();
        let setup = Setup { plane: Plane::Horizontal, height: 0.0, margin: 5.0, n: 41 };
        let m = compute(&c.near, &a, a.freq_mhz, &setup).unwrap();
        assert_eq!(m.eh.len(), m.nu * m.nv);
        // 100 W of carrier into a 14 MHz dipole: over ICNIRP 1998's 28 V/m close in,
        // within it a few metres out
        let s = summarise(&m, 100.0, Standard::Icnirp1998).unwrap();
        let d = s.over_to.expect("over the limit next to the wire");
        assert!(d > 0.3 && d < 5.0, "{d}");
        // a milliwatt is nowhere near
        assert!(summarise(&m, 0.001, Standard::Icnirp1998).unwrap().over_to.is_none());
    }
}
