//! Tools → Great circle map: the antenna's horizontal pattern laid over an azimuthal
//! equidistant world map centred on the station, so its beam points where it does on the
//! ground. The station is given as a Maidenhead locator; the user marks which way North
//! lies on the antenna's top view (the bearing of the model's +x axis). Both are kept in
//! the settings file. Coastlines: Natural Earth 1:110m (public domain), packed by
//! tools/coastlines.py.

use crate::mom::{FullResult, Gain};
use crate::model::Antenna;
use crate::plots::{radius_frac, GRID, INK, PLOT_BG, TOTAL};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Shape, Stroke, StrokeKind, Vec2};
use std::f64::consts::PI;

/// Mean radius of the Earth, km.
pub const EARTH_KM: f64 = 6371.0;

// ------------------------------------------------------------------ locators and geometry

/// Latitude and longitude (degrees) of the centre of a Maidenhead locator square of
/// 2, 4, 6 or 8 characters ("IO91", "io91wm", "JN58td25").
pub fn locator_to_latlon(s: &str) -> Option<(f64, f64)> {
    let c: Vec<u8> = s.trim().bytes().collect();
    if c.is_empty() || !c.len().is_multiple_of(2) || c.len() > 8 {
        return None;
    }
    let (mut lon, mut lat) = (-180.0, -90.0);
    let (mut w, mut h) = (360.0, 180.0); // size of the current cell
    for (i, pair) in c.chunks(2).enumerate() {
        let (div, base, top) = match i {
            0 => (18.0, b'A', b'R'),
            1 | 3 => (10.0, b'0', b'9'),
            _ => (24.0, b'A', b'X'),
        };
        let a = pair[0].to_ascii_uppercase();
        let b = pair[1].to_ascii_uppercase();
        if !(base..=top).contains(&a) || !(base..=top).contains(&b) {
            return None;
        }
        w /= div;
        h /= div;
        lon += (a - base) as f64 * w;
        lat += (b - base) as f64 * h;
    }
    Some((lat + h / 2.0, lon + w / 2.0))
}

/// The six-character locator ("IO91wm") of a place.
pub fn latlon_to_locator(lat: f64, lon: f64) -> String {
    let lon = (lon + 180.0).rem_euclid(360.0);
    let lat = (lat + 90.0).clamp(0.0, 179.9999);
    let f = |v: f64, cell: f64, n: u32| ((v / cell).floor() as u32).min(n - 1);
    let (a, b) = (f(lon, 20.0, 18), f(lat, 10.0, 18));
    let (lon, lat) = (lon - a as f64 * 20.0, lat - b as f64 * 10.0);
    let (c, d) = (f(lon, 2.0, 10), f(lat, 1.0, 10));
    let (lon, lat) = (lon - c as f64 * 2.0, lat - d as f64 * 1.0);
    let (e, g) = (f(lon, 2.0 / 24.0, 24), f(lat, 1.0 / 24.0, 24));
    let ch = |base: u8, i: u32| (base + i as u8) as char;
    [ch(b'A', a), ch(b'A', b), ch(b'0', c), ch(b'0', d), ch(b'a', e), ch(b'a', g)].iter().collect()
}

/// Initial bearing (degrees from true north, clockwise) and angular distance (radians)
/// along the great circle from one place to another.
pub fn bearing_distance(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let (p1, l1) = (from.0.to_radians(), from.1.to_radians());
    let (p2, l2) = (to.0.to_radians(), to.1.to_radians());
    let dl = l2 - l1;
    let brg = (dl.sin() * p2.cos()).atan2(p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos());
    // haversine, good at short range; clamped for the antipode
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let d = 2.0 * h.sqrt().min(1.0).asin();
    (brg.to_degrees().rem_euclid(360.0), d)
}

/// The place reached going `dist` radians along bearing `brg` (degrees) from `from`.
pub fn destination(from: (f64, f64), brg: f64, dist: f64) -> (f64, f64) {
    let (p1, l1) = (from.0.to_radians(), from.1.to_radians());
    let t = brg.to_radians();
    let p2 = (p1.sin() * dist.cos() + p1.cos() * dist.sin() * t.cos()).clamp(-1.0, 1.0).asin();
    let l2 = l1 + (t.sin() * dist.sin() * p1.cos()).atan2(dist.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), (l2.to_degrees() + 180.0).rem_euclid(360.0) - 180.0)
}

/// Azimuthal equidistant projection about `centre`: (east, north) with the antipode at
/// radius 1.
pub fn project(centre: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    let (b, d) = bearing_distance(centre, p);
    let r = d / PI;
    let b = b.to_radians();
    (r * b.sin(), r * b.cos())
}

/// Compass bearing of the model's azimuth `az` (degrees anticlockwise from +x, seen from
/// above) when the +x axis points to `x_bearing`.
pub fn compass(az: f64, x_bearing: f64) -> f64 {
    (x_bearing - az).rem_euclid(360.0)
}

/// The coastlines as lines of (latitude, longitude).
pub fn coastlines() -> &'static [Vec<(f64, f64)>] {
    static C: std::sync::OnceLock<Vec<Vec<(f64, f64)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let b = include_bytes!("coast110.bin");
        let rd = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]);
        let mut out = Vec::new();
        let mut i = 0;
        while i + 2 <= b.len() {
            let n = u16::from_le_bytes([b[i], b[i + 1]]) as usize;
            i += 2;
            let mut line = Vec::with_capacity(n);
            for _ in 0..n {
                line.push((rd(i + 2) as f64 / 100.0, rd(i) as f64 / 100.0));
                i += 4;
            }
            out.push(line);
        }
        out
    })
}

/// Gain in the azimuth cut nearest model azimuth `az`.
fn gain_at(cut: &[(f64, Gain)], az: f64) -> Option<f64> {
    let az = az.rem_euclid(360.0);
    cut.iter()
        .min_by(|a, b| {
            let d = |x: f64| {
                let d = (x - az).rem_euclid(360.0);
                d.min(360.0 - d)
            };
            d(a.0).total_cmp(&d(b.0))
        })
        .map(|(_, g)| g.total)
}

// ------------------------------------------------------------------ the window

pub struct GreatCircleState {
    pub open: bool,
    locator: String,
    /// compass bearing of the model's +x axis
    x_bearing: f64,
    /// radius of the pattern's 0 dB ring, as a fraction of the map's
    pattern_scale: f32,
}

impl Default for GreatCircleState {
    fn default() -> Self {
        let s = crate::settings::get();
        GreatCircleState { open: false, locator: s.locator, x_bearing: s.x_bearing, pattern_scale: 0.6 }
    }
}

impl GreatCircleState {
    fn set_x_bearing(&mut self, b: f64) {
        self.x_bearing = (b.rem_euclid(360.0) * 10.0).round() / 10.0 % 360.0;
        let b = self.x_bearing;
        crate::settings::set(|s| s.x_bearing = b);
    }
}

const SEA: Color32 = Color32::from_rgb(232, 240, 248);
const COAST: Color32 = Color32::from_rgb(70, 110, 80);
const BEAM: Color32 = Color32::from_rgb(30, 80, 200);

pub fn window(ctx: &egui::Context, st: &mut GreatCircleState, ant: &Antenna, calc: Option<&FullResult>) {
    if !st.open {
        return;
    }
    let mut open = st.open;
    egui::Window::new("Great circle map").open(&mut open).default_width(860.0).show(ctx, |ui| {
        let home = locator_to_latlon(&st.locator);
        let beam_az = calc.map(|c| c.result.az_max);
        ui.horizontal(|ui| {
            ui.label("Locator");
            let r = ui.add(egui::TextEdit::singleline(&mut st.locator).desired_width(80.0).hint_text("IO91wm"));
            if r.changed() {
                let l = st.locator.trim().to_string();
                crate::settings::set(|s| s.locator = l);
            }
            match home {
                Some((lat, lon)) => {
                    ui.label(RichText::new(format!("{:.2}°{} {:.2}°{}", lat.abs(), if lat >= 0.0 { "N" } else { "S" }, lon.abs(), if lon >= 0.0 { "E" } else { "W" })).color(Color32::GRAY));
                }
                None => {
                    ui.colored_label(Color32::from_rgb(190, 60, 30), "Type your Maidenhead locator (4 or 6 characters, e.g. IO91wm).");
                }
            }
            ui.separator();
            ui.label("+x axis points to");
            let mut b = st.x_bearing;
            if ui.add(egui::DragValue::new(&mut b).speed(1.0).range(-360.0..=720.0).suffix("°")).on_hover_text("Compass bearing of the model's +x axis. You can also drag the N arrow on the top view, or click the map to aim the beam there.").changed() {
                st.set_x_bearing(b);
            }
            ui.separator();
            ui.label("Pattern size");
            ui.add(egui::Slider::new(&mut st.pattern_scale, 0.2..=1.0).show_value(false));
        });
        match (calc, beam_az) {
            (Some(c), Some(az)) => {
                let b = compass(az, st.x_bearing);
                let bw = c.patterns.az_beamwidth().map_or(String::new(), |w| format!(", −3 dB beamwidth {w:.0}°"));
                ui.label(format!(
                    "Beam heading {b:.0}° (long path {:.0}°), {:.2} dBi at {:.0}° elevation{bw}. Click the map to aim the beam at a place.",
                    (b + 180.0) % 360.0,
                    c.result.ga_dbi,
                    c.patterns.el_cut_deg
                ));
            }
            _ => {
                ui.label(RichText::new("Calculate the antenna to see its pattern on the map.").color(Color32::GRAY));
            }
        }
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            top_view(ui, st, ant);
            world_map(ui, st, home, calc);
        });
    });
    st.open = open;
}

/// The antenna from above, with a North arrow the user drags to say which way North lies.
fn top_view(ui: &mut egui::Ui, st: &mut GreatCircleState, ant: &Antenna) {
    let size = 240.0;
    let (resp, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, PLOT_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    painter.text(rect.left_top() + Vec2::new(8.0, 4.0), Align2::LEFT_TOP, "Top view", FontId::proportional(14.0), INK);
    let c = rect.center() + Vec2::new(0.0, 8.0);
    let r0 = size * 0.36;
    // fit the wires, centred on their middle
    let pts: Vec<[f64; 2]> = ant.wires.iter().flat_map(|w| [[w.p1[0], w.p1[1]], [w.p2[0], w.p2[1]]]).collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if !pts.is_empty() {
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let half = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / 2.0).max(1e-6);
        let k = r0 as f64 * 0.95 / half;
        let to = |p: [f64; 3]| c + Vec2::new(((p[0] - mid[0]) * k) as f32, (-(p[1] - mid[1]) * k) as f32);
        for w in &ant.wires {
            painter.line_segment([to(w.p1), to(w.p2)], Stroke::new(2.0, INK));
        }
    }
    // model axes
    let small = FontId::proportional(11.0);
    let ax = rect.left_bottom() + Vec2::new(14.0, -14.0);
    painter.arrow(ax, Vec2::new(24.0, 0.0), Stroke::new(1.0, Color32::GRAY));
    painter.arrow(ax, Vec2::new(0.0, -24.0), Stroke::new(1.0, Color32::GRAY));
    painter.text(ax + Vec2::new(28.0, 0.0), Align2::LEFT_CENTER, "x", small.clone(), Color32::GRAY);
    painter.text(ax + Vec2::new(0.0, -28.0), Align2::CENTER_BOTTOM, "y", small.clone(), Color32::GRAY);
    // North lies at model azimuth x_bearing
    if (resp.dragged() || resp.clicked()) && let Some(p) = resp.interact_pointer_pos() {
        let d = p - c;
        if d.length() > 6.0 {
            st.set_x_bearing((-d.y as f64).atan2(d.x as f64).to_degrees());
        }
    }
    let a = st.x_bearing.to_radians() as f32;
    let dir = Vec2::new(a.cos(), -a.sin());
    let tip = c + dir * (r0 + 14.0);
    painter.arrow(c, dir * (r0 + 14.0), Stroke::new(2.0, BEAM));
    painter.circle_filled(tip + dir * 9.0, 8.0, BEAM);
    painter.text(tip + dir * 9.0, Align2::CENTER_CENTER, "N", FontId::proportional(11.0), Color32::WHITE);
    resp.on_hover_text("Drag the N arrow to the direction of North.");
}

fn world_map(ui: &mut egui::Ui, st: &mut GreatCircleState, home: Option<(f64, f64)>, calc: Option<&FullResult>) {
    let size = (ui.available_width() - 4.0).clamp(360.0, 640.0);
    let (resp, painter) = ui.allocate_painter(Vec2::new(size, size + 20.0), Sense::click());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, PLOT_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    let c = Pos2::new(rect.center().x, rect.top() + size / 2.0);
    let r0 = size / 2.0 - 22.0;
    painter.circle_filled(c, r0, SEA);
    let Some(home) = home else {
        painter.text(c, Align2::CENTER_CENTER, "Type your locator above", FontId::proportional(15.0), Color32::GRAY);
        return;
    };
    let to = |xy: (f64, f64)| c + Vec2::new(xy.0 as f32, -(xy.1 as f32)) * r0;
    // distance rings every 5000 km and bearing lines every 30°
    let small = FontId::proportional(10.0);
    for km in [5000.0, 10000.0, 15000.0] {
        let rr = r0 * (km / EARTH_KM / PI) as f32;
        painter.circle_stroke(c, rr, Stroke::new(0.6, GRID));
        painter.text(c + Vec2::new(3.0, -rr), Align2::LEFT_BOTTOM, format!("{} km", km as u32), small.clone(), Color32::GRAY);
    }
    for k in 0..12 {
        let a = (k as f32 * 30.0).to_radians();
        let d = Vec2::new(a.sin(), -a.cos());
        painter.line_segment([c, c + d * r0], Stroke::new(0.5, GRID));
        let lab = match k {
            0 => "N".to_string(),
            3 => "E".to_string(),
            6 => "S".to_string(),
            9 => "W".to_string(),
            _ => format!("{}", k * 30),
        };
        painter.text(c + d * (r0 + 10.0), Align2::CENTER_CENTER, lab, small.clone(), Color32::DARK_GRAY);
    }
    // coastlines; a segment that jumps across the map lies near the antipode
    for line in coastlines() {
        let mut run: Vec<Pos2> = Vec::new();
        let mut last: Option<(f64, f64)> = None;
        for &p in line {
            let xy = project(home, p);
            if last.is_some_and(|l| ((xy.0 - l.0).powi(2) + (xy.1 - l.1).powi(2)).sqrt() > 0.2) {
                if run.len() > 1 {
                    painter.add(Shape::line(std::mem::take(&mut run), Stroke::new(1.0, COAST)));
                }
                run.clear();
            }
            run.push(to(xy));
            last = Some(xy);
        }
        if run.len() > 1 {
            painter.add(Shape::line(run, Stroke::new(1.0, COAST)));
        }
    }
    painter.circle_stroke(c, r0, Stroke::new(1.2, GRID));
    // the pattern, turned to the compass
    if let Some(calc) = calc {
        let cut: Vec<&(f64, Gain)> = calc.patterns.az_cut.iter().filter(|(a, _)| *a < 360.0).collect();
        let gmax = cut.iter().map(|(_, g)| g.total).fold(0.0f64, f64::max);
        if gmax > 0.0 && cut.len() > 2 {
            let maxdb = 10.0 * gmax.log10();
            let rp = r0 * st.pattern_scale;
            let pts: Vec<Pos2> = cut
                .iter()
                .map(|(az, g)| {
                    let rel = if g.total > 0.0 { 10.0 * g.total.log10() - maxdb } else { -100.0 };
                    let b = compass(*az, st.x_bearing).to_radians() as f32;
                    c + Vec2::new(b.sin(), -b.cos()) * rp * radius_frac(rel)
                })
                .collect();
            // a polar pattern is star-shaped about its centre, so a fan fills it
            let fill = Color32::from_rgba_unmultiplied(200, 30, 40, 50);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(c, fill);
            for p in &pts {
                mesh.colored_vertex(*p, fill);
            }
            let n = pts.len() as u32;
            for i in 0..n {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
            }
            painter.add(Shape::mesh(mesh));
            let mut closed = pts.clone();
            closed.push(pts[0]);
            painter.add(Shape::line(closed, Stroke::new(1.5, TOTAL)));
            // the beam's great circle, short path solid, long path dashed
            let b = compass(calc.result.az_max, st.x_bearing).to_radians() as f32;
            let d = Vec2::new(b.sin(), -b.cos());
            painter.line_segment([c, c + d * r0], Stroke::new(1.5, BEAM));
            painter.extend(Shape::dashed_line(&[c, c - d * r0], Stroke::new(1.0, BEAM), 6.0, 4.0));
        }
    }
    painter.circle_filled(c, 3.5, BEAM);
    // hover: where the pointer is; click: aim the beam there
    let mut info = String::new();
    if let Some(p) = resp.hover_pos() {
        let v = p - c;
        let r = v.length() / r0;
        if r <= 1.0 {
            let brg = (v.x as f64).atan2(-v.y as f64).to_degrees().rem_euclid(360.0);
            let dist = r as f64 * PI;
            let (lat, lon) = destination(home, brg, dist);
            info = format!("{}  bearing {brg:.0}°  {:.0} km", latlon_to_locator(lat, lon), dist * EARTH_KM);
            if let Some(calc) = calc {
                let az = st.x_bearing - brg;
                let cut = &calc.patterns.az_cut;
                if let (Some(g), Some(gmax)) = (gain_at(cut, az), cut.iter().map(|(_, g)| g.total).reduce(f64::max))
                    && g > 0.0
                    && gmax > 0.0
                {
                    info += &format!("  {:.1} dBi ({:+.1} dB)", 10.0 * g.log10(), 10.0 * (g / gmax).log10());
                }
                if resp.clicked() {
                    st.set_x_bearing(brg + calc.result.az_max);
                }
            }
        }
    }
    painter.text(Pos2::new(rect.left() + 8.0, rect.bottom() - 6.0), Align2::LEFT_BOTTOM, info, FontId::proportional(12.0), INK);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locators() {
        // IO91wm: central London, centre 51.52 N 0.125 W
        let (lat, lon) = locator_to_latlon("IO91wm").unwrap();
        assert!((lat - 51.5208).abs() < 1e-3 && (lon + 0.125).abs() < 1e-3, "{lat} {lon}");
        assert_eq!(latlon_to_locator(lat, lon), "IO91wm");
        // FN31pr: Newington, Connecticut
        let (lat, lon) = locator_to_latlon("fn31PR").unwrap();
        assert_eq!(latlon_to_locator(lat, lon), "FN31pr");
        assert!((lat - 41.729).abs() < 0.01 && (lon + 72.708).abs() < 0.01, "{lat} {lon}");
        // a square's centre
        assert_eq!(locator_to_latlon("JN58"), Some((48.5, 11.0)));
        assert_eq!(locator_to_latlon("JN58td25").map(|(a, b)| latlon_to_locator(a, b)), Some("JN58td".into()));
        assert!(locator_to_latlon("ZZ00").is_none());
        assert!(locator_to_latlon("IO9").is_none());
        assert!(locator_to_latlon("").is_none());
        // the far corner stays in range
        assert_eq!(latlon_to_locator(90.0, 180.0), "AR09ax");
    }

    #[test]
    fn great_circles() {
        let london = (51.5, -0.12);
        let ny = (40.71, -74.0);
        let (b, d) = bearing_distance(london, ny);
        assert!((b - 288.3).abs() < 0.5, "{b}");
        assert!((d * EARTH_KM - 5570.0).abs() < 20.0, "{}", d * EARTH_KM);
        // going there along that bearing arrives
        let (lat, lon) = destination(london, b, d);
        assert!((lat - ny.0).abs() < 1e-6 && (lon - ny.1).abs() < 1e-6, "{lat} {lon}");
        // due east along the equator
        let (lat, lon) = destination((0.0, 0.0), 90.0, PI / 2.0);
        assert!(lat.abs() < 1e-9 && (lon - 90.0).abs() < 1e-9);
        // projection: north is up, the antipode at the rim
        let (x, y) = project((0.0, 0.0), (45.0, 0.0));
        assert!(x.abs() < 1e-9 && (y - 0.25).abs() < 1e-9);
        let (x, y) = project((10.0, 20.0), (-10.0, -160.0));
        assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compass_bearings() {
        // +x north: model azimuth 90 (+y) is west
        assert_eq!(compass(0.0, 0.0), 0.0);
        assert_eq!(compass(90.0, 0.0), 270.0);
        // +x east: +y is north
        assert_eq!(compass(90.0, 90.0), 0.0);
    }

    #[test]
    fn coastlines_load() {
        let c = coastlines();
        assert_eq!(c.len(), 134);
        assert!(c.iter().flatten().all(|(lat, lon)| lat.abs() <= 90.0 && lon.abs() <= 180.0));
    }
}
