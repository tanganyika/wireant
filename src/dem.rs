//! Terrain profiles from elevation data: the AWS Terrain Tiles (Mapzen's
//! "Terrarium" PNG tiles, built from SRTM, USGS 3DEP and others), downloaded once
//! into a cache folder and sampled along bearings from the site.
//!
//! Each tile is a 256 × 256 PNG in the web-map (Web Mercator) tiling; a pixel's
//! height is R·256 + G + B/256 − 32768 metres.  Profiles run from the site out to
//! a radius along bearings measured in the model's azimuths (0° = +X, toward +Y),
//! turned onto the compass with the great circle tool's bearing of +X.  Heights
//! are relative to the ground at the site and drop with the earth's curvature
//! (4/3 earth radius, for ordinary refraction), then are thinned to the points
//! needed to stay within 1 m of the profile (well inside the data's accuracy).

use crate::greatcircle::{compass, destination, EARTH_KM};
use crate::model::TerrainProfile;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Zoom level fetched: pixels about 38 m across at the equator, 25 m at 50°
/// latitude; the data underneath is mostly 30 m (SRTM) outside the USA.
pub const ZOOM: u32 = 12;
const URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
/// The sources the tiles' terms ask to be credited.
pub const ATTRIBUTION: &str = "Elevation data: Terrain Tiles (Mapzen / Tilezen, on the AWS Registry of Open Data), from \
     SRTM (NASA), USGS 3DEP and GMTED2010, ETOPO1 (NOAA), Geoscience Australia, Canada CDEM, EU-DEM (Copernicus), \
     Kartverket (Norway), LINZ (New Zealand) and others.";
/// Effective earth radius factor for refraction in an ordinary atmosphere.
const K_FACTOR: f64 = 4.0 / 3.0;

/// What to fetch.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// latitude and longitude of the antenna's origin, degrees
    pub site: (f64, f64),
    /// compass bearing (degrees) of the model's +X axis
    pub x_bearing: f64,
    /// profiles all round, equally spaced
    pub bearings: usize,
    /// out to (m)
    pub radius_m: f64,
    /// sample spacing along each profile (m)
    pub step_m: f64,
}

impl Default for Request {
    fn default() -> Self {
        Request { site: (0.0, 0.0), x_bearing: 0.0, bearings: 36, radius_m: 5000.0, step_m: 30.0 }
    }
}

/// The result: profiles ready for the Terrain card.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub profiles: Vec<TerrainProfile>,
    /// ground height at the site above sea level (m)
    pub site_height: f64,
    pub tiles: usize,
    pub downloaded: usize,
}

/// Fractional tile coordinates of a place at zoom `z`.
pub fn tile_xy(lat: f64, lon: f64, z: u32) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let lat = lat.clamp(-85.05, 85.05).to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n)
}

/// The heights in one decoded tile, row by row.
fn decode(png: &[u8]) -> Result<Vec<f32>, String> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|e| format!("bad elevation tile: {e}"))?.to_rgb8();
    if img.dimensions() != (256, 256) {
        return Err(format!("elevation tile is {:?}, not 256 × 256", img.dimensions()));
    }
    Ok(img.pixels().map(|p| (p[0] as f32 * 256.0 + p[1] as f32 + p[2] as f32 / 256.0) - 32768.0).collect())
}

/// Where tiles are kept: the program's cache folder (`WIREANT_CACHE_DIR` overrides).
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("WIREANT_CACHE_DIR") {
        return Some(PathBuf::from(d).join("terrarium"));
    }
    dirs::cache_dir().map(|d| d.join("wireant").join("terrarium"))
}

/// Download one tile.
pub fn download(z: u32, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let url = format!("{URL}/{z}/{x}/{y}.png");
    let mut r = ureq::get(&url).header("User-Agent", concat!("wireant/", env!("CARGO_PKG_VERSION"))).call().map_err(|e| format!("Could not download elevation data ({url}): {e}"))?;
    r.body_mut().with_config().limit(4 << 20).read_to_vec().map_err(|e| format!("Could not download elevation data ({url}): {e}"))
}

/// A tile's place, its heights, and whether it was downloaded (not from the cache).
type Loaded = ((u32, u32), Vec<f32>, bool);

/// Tiles held in memory, loaded from the cache or fetched.
struct Grid {
    z: u32,
    tiles: HashMap<(u32, u32), Vec<f32>>,
}

impl Grid {
    /// Height at global pixel (px, py), x wrapping round the world.
    fn pixel(&self, px: i64, py: i64) -> f64 {
        let n = 256i64 << self.z;
        let px = px.rem_euclid(n);
        let py = py.clamp(0, n - 1);
        let t = &self.tiles[&((px / 256) as u32, (py / 256) as u32)];
        t[(py % 256 * 256 + px % 256) as usize] as f64
    }

    /// Height at a place, bilinear between pixel centres.
    fn height(&self, lat: f64, lon: f64) -> f64 {
        let (x, y) = tile_xy(lat, lon, self.z);
        let (gx, gy) = (x * 256.0 - 0.5, y * 256.0 - 0.5);
        let (ix, iy) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - ix, gy - iy);
        let (ix, iy) = (ix as i64, iy as i64);
        let h = |dx, dy| self.pixel(ix + dx, iy + dy);
        (h(0, 0) * (1.0 - fx) + h(1, 0) * fx) * (1.0 - fy) + (h(0, 1) * (1.0 - fx) + h(1, 1) * fx) * fy
    }
}

/// The places sampled along one bearing (model azimuth, degrees).
fn samples(req: &Request, az: f64) -> Vec<(f64, (f64, f64))> {
    let n = (req.radius_m / req.step_m).ceil().max(1.0) as usize;
    let brg = compass(az, req.x_bearing);
    (0..=n).map(|i| (i as f64 * req.radius_m / n as f64, destination(req.site, brg, i as f64 * req.radius_m / n as f64 / (EARTH_KM * 1000.0)))).collect()
}

/// Ramer–Douglas–Peucker: the points needed to keep within `tol` metres of the profile.
fn thin(p: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    fn go(p: &[[f64; 2]], tol: f64, keep: &mut [bool], lo: usize, hi: usize) {
        if hi <= lo + 1 {
            return;
        }
        let (a, b) = (p[lo], p[hi]);
        let slope = (b[1] - a[1]) / (b[0] - a[0]);
        let (mut worst, mut at) = (0.0, lo);
        for (i, q) in p.iter().enumerate().take(hi).skip(lo + 1) {
            let d = (q[1] - (a[1] + slope * (q[0] - a[0]))).abs();
            if d > worst {
                (worst, at) = (d, i);
            }
        }
        if worst > tol {
            keep[at] = true;
            go(p, tol, keep, lo, at);
            go(p, tol, keep, at, hi);
        }
    }
    if p.len() <= 2 {
        return p.to_vec();
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    go(p, tol, &mut keep, 0, p.len() - 1);
    p.iter().zip(keep).filter(|(_, k)| *k).map(|(q, _)| *q).collect()
}

/// Profiles round the site, fetching tiles with `fetch` (z, x, y → PNG bytes) when
/// the cache in `cache` (if any) doesn't have them.
pub fn profiles_with(req: &Request, cache: Option<PathBuf>, fetch: &(dyn Fn(u32, u32, u32) -> Result<Vec<u8>, String> + Sync)) -> Result<Fetched, String> {
    let (lat, lon) = req.site;
    if !(-85.0..=85.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(format!("The site ({lat:.4}, {lon:.4}) is outside the elevation data (latitudes 85°S to 85°N)."));
    }
    if req.bearings == 0 || req.radius_m <= 0.0 || req.step_m <= 0.0 {
        return Err("Need at least one bearing and a radius and step above zero.".into());
    }
    let z = ZOOM;
    let azs: Vec<f64> = (0..req.bearings).map(|i| i as f64 * 360.0 / req.bearings as f64).collect();
    let all: Vec<Vec<(f64, (f64, f64))>> = azs.iter().map(|&az| samples(req, az)).collect();
    // every tile a sample's four pixels touch
    let n = 256i64 << z;
    let mut need = std::collections::BTreeSet::new();
    for (la, lo) in all.iter().flatten().map(|s| s.1) {
        let (x, y) = tile_xy(la, lo, z);
        let (gx, gy) = ((x * 256.0 - 0.5).floor() as i64, (y * 256.0 - 0.5).floor() as i64);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (px, py) = ((gx + dx).rem_euclid(n), (gy + dy).clamp(0, n - 1));
            need.insert(((px / 256) as u32, (py / 256) as u32));
        }
    }
    let need: Vec<(u32, u32)> = need.into_iter().collect();
    let loaded: Vec<Result<Loaded, String>> = need
        .par_iter()
        .map(|&(x, y)| {
            let file = cache.as_ref().map(|d| d.join(z.to_string()).join(x.to_string()).join(format!("{y}.png")));
            if let Some(h) = file.as_ref().and_then(|f| std::fs::read(f).ok()).and_then(|b| decode(&b).ok()) {
                return Ok(((x, y), h, false));
            }
            let bytes = fetch(z, x, y)?;
            let h = decode(&bytes)?;
            if let Some(f) = file {
                // a cache that can't be written only costs a download next time
                let _ = std::fs::create_dir_all(f.parent().unwrap()).and_then(|_| std::fs::write(&f, &bytes));
            }
            Ok(((x, y), h, true))
        })
        .collect();
    let mut grid = Grid { z, tiles: HashMap::new() };
    let mut downloaded = 0;
    for r in loaded {
        let (k, h, fresh) = r?;
        downloaded += fresh as usize;
        grid.tiles.insert(k, h);
    }
    let h0 = grid.height(lat, lon);
    let re = K_FACTOR * EARTH_KM * 1000.0;
    let profiles = azs
        .iter()
        .zip(&all)
        .map(|(&az, s)| {
            let pts: Vec<[f64; 2]> = s.iter().map(|&(d, (la, lo))| [d, grid.height(la, lo) - h0 - d * d / (2.0 * re)]).collect();
            let mut pts = thin(&pts, 1.0);
            for q in &mut pts {
                q[0] = (q[0] * 10.0).round() / 10.0;
                q[1] = (q[1] * 10.0).round() / 10.0;
            }
            TerrainProfile { bearing_deg: az, points: pts }
        })
        .collect();
    Ok(Fetched { profiles, site_height: h0, tiles: need.len(), downloaded })
}

/// Profiles round the site, from the cache or downloaded.
pub fn profiles(req: &Request) -> Result<Fetched, String> {
    profiles_with(req, cache_dir(), &download)
}

/// The saved site's terrain (Preferences: site, +X bearing, profiles and radius)
/// from the tiles already in the cache, without the network: None when no site
/// is saved, an error when its tiles haven't been downloaded yet.
pub fn from_cache() -> Option<Result<(Fetched, Request), String>> {
    let s = crate::settings::get();
    let site = parse_site(&s.site)?;
    let req = Request { site, x_bearing: s.x_bearing, bearings: s.terrain_bearings, radius_m: s.terrain_radius_km * 1000.0, step_m: 30.0 };
    Some(profiles_with(&req, cache_dir(), &|_, _, _| Err("not cached".into())).map(|f| (f, req)))
}

/// A site typed as "lat, lon" in decimal degrees (south and west negative, or with
/// N/S/E/W after the numbers) or as a Maidenhead locator of 6 to 10 characters.
pub fn parse_site(s: &str) -> Option<(f64, f64)> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.len() == 2 {
        let num = |p: &str, pos: char, neg: char| -> Option<f64> {
            let p = p.trim_end_matches('°');
            let last = p.chars().last()?.to_ascii_uppercase();
            if last == pos || last == neg {
                let v: f64 = p[..p.len() - 1].trim_end_matches('°').parse().ok()?;
                Some(if last == neg { -v } else { v })
            } else {
                p.parse().ok()
            }
        };
        let (lat, lon) = (num(parts[0], 'N', 'S')?, num(parts[1], 'E', 'W')?);
        return ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon));
    }
    if parts.len() == 1 && s.len() >= 6 {
        return locator10(s);
    }
    None
}

/// Centre of a Maidenhead locator of 6, 8 or 10 characters (the 10th pair is
/// letters again, 24 to the subsquare's tenth).
fn locator10(s: &str) -> Option<(f64, f64)> {
    let c = s.as_bytes();
    if c.len() <= 8 {
        return crate::greatcircle::locator_to_latlon(s);
    }
    if c.len() != 10 {
        return None;
    }
    let (lat8, lon8) = crate::greatcircle::locator_to_latlon(&s[..8])?;
    // the 8-character square is 2'/24/10 by 1°/24/10; step back to its corner
    let (w8, h8) = (2.0 / 240.0, 1.0 / 240.0);
    let (a, b) = (c[8].to_ascii_uppercase(), c[9].to_ascii_uppercase());
    if !(b'A'..=b'X').contains(&a) || !(b'A'..=b'X').contains(&b) {
        return None;
    }
    let (w, h) = (w8 / 24.0, h8 / 24.0);
    Some((lat8 - h8 / 2.0 + (b - b'A') as f64 * h + h / 2.0, lon8 - w8 / 2.0 + (a - b'A') as f64 * w + w / 2.0))
}

// ------------------------------------------------------------------ the dialog

use eframe::egui::{self, Color32, RichText};

/// The "Terrain from elevation data" window.
pub struct FetchState {
    pub open: bool,
    site: String,
    bearings: usize,
    radius_km: f64,
    job: Option<std::sync::mpsc::Receiver<Result<Fetched, String>>>,
    error: Option<String>,
}

impl Default for FetchState {
    fn default() -> Self {
        let s = crate::settings::get();
        // a locator fine enough to start from, else nothing
        let site = if !s.site.is_empty() { s.site } else if s.locator.trim().len() >= 8 { s.locator } else { String::new() };
        FetchState { open: false, site, bearings: s.terrain_bearings, radius_km: s.terrain_radius_km, job: None, error: None }
    }
}

fn latlon_text((lat, lon): (f64, f64)) -> String {
    format!("{:.5}°{} {:.5}°{}", lat.abs(), if lat >= 0.0 { "N" } else { "S" }, lon.abs(), if lon >= 0.0 { "E" } else { "W" })
}

/// Size (m, north–south by east–west) of the square a locator names, or None for
/// latitude and longitude.
fn locator_cell_m(site: &str, lat: f64) -> Option<(f64, f64)> {
    let t = site.trim();
    if t.contains(|c: char| c == ',' || c.is_whitespace() || c == '.') {
        return None;
    }
    let deg = match t.len() {
        6 => (1.0 / 24.0, 2.0 / 24.0),
        8 => (1.0 / 240.0, 2.0 / 240.0),
        10 => (1.0 / 5760.0, 2.0 / 5760.0),
        _ => return None,
    };
    let m = EARTH_KM * 1000.0 * std::f64::consts::PI / 180.0;
    Some((deg.0 * m, deg.1 * m * lat.to_radians().cos()))
}

/// Draws the window. Returns the profiles once fetched, for the Terrain card.
pub fn window(ctx: &egui::Context, st: &mut FetchState, gc: &mut crate::greatcircle::GreatCircleState) -> Option<Fetched> {
    let mut got = None;
    if let Some(rx) = &st.job {
        match rx.try_recv() {
            Ok(Ok(f)) => {
                st.job = None;
                st.open = false;
                got = Some(f);
            }
            Ok(Err(e)) => {
                st.job = None;
                st.error = Some(e);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                st.job = None;
                st.error = Some("The elevation data download stopped.".into());
            }
        }
    }
    if !st.open {
        return got;
    }
    let mut open = st.open;
    egui::Window::new("Terrain from elevation data").open(&mut open).default_width(520.0).collapsible(false).show(ctx, |ui| {
        ui.label(
            RichText::new(
                "Profiles of the ground round the antenna, from map elevation data, as Radio Mobile and HFTA use. They replace the \
                 Terrain card's profiles. The data is the ground's surface, partly including trees and buildings, on a grid about \
                 30 m across, so it smooths sharp peaks and small banks.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let site = parse_site(&st.site);
        egui::Grid::new("dem_grid").spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Antenna at");
            ui.vertical(|ui| {
                if ui.add(egui::TextEdit::singleline(&mut st.site).desired_width(260.0).hint_text("51.5072, -0.1276")).changed() {
                    st.error = None;
                }
                match site {
                    Some(p) => {
                        let mut t = latlon_text(p);
                        let mut warn = false;
                        if let Some((ns, ew)) = locator_cell_m(&st.site, p.0) {
                            t += &format!(", the centre of a {ns:.0} × {ew:.0} m square");
                            warn = ns > 100.0;
                        }
                        ui.label(RichText::new(t).color(Color32::GRAY));
                        if warn {
                            ui.colored_label(Color32::from_rgb(190, 110, 0), "Too coarse for the ground near the antenna: give latitude and longitude from a map, or a 10-character locator.");
                        }
                    }
                    None => {
                        let t = "Latitude and longitude in degrees (south and west negative; most map sites show them when you right-click a place), or a 10-character Maidenhead locator.";
                        if st.site.trim().is_empty() {
                            ui.label(RichText::new(t).weak());
                        } else {
                            ui.colored_label(Color32::from_rgb(190, 60, 30), t);
                        }
                    }
                }
            });
            ui.end_row();
            ui.label("+X axis points to");
            let mut b = gc.x_bearing();
            ui.horizontal(|ui| {
                if ui.add(crate::num::Num::new(&mut b).speed(1.0).range(-360.0..=720.0).suffix("°")).on_hover_text("Compass bearing of the model's +X axis (the same setting as the Great circle map's).").changed() {
                    gc.set_x_bearing(b);
                }
                ui.label(RichText::new("compass bearing, shared with the Great circle map").weak());
            });
            ui.end_row();
            ui.label("Profiles");
            egui::ComboBox::from_id_salt("dem_bearings").selected_text(format!("{}, every {}°", st.bearings, 360 / st.bearings)).show_ui(ui, |ui| {
                for n in [8, 12, 18, 24, 36, 72] {
                    ui.selectable_value(&mut st.bearings, n, format!("{n}, every {}°", 360 / n));
                }
            });
            ui.end_row();
            ui.label("Out to");
            ui.add(crate::num::Num::new(&mut st.radius_km).speed(0.1).range(0.5..=30.0).suffix(" km")).on_hover_text(
                "How far each profile runs. Reflections at low angles come from up to a few km away; hills further out cut the lowest rays.",
            );
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let can = site.is_some() && st.job.is_none();
            if ui.add_enabled(can, egui::Button::new("Fetch terrain")).clicked() {
                let req = Request { site: site.unwrap(), x_bearing: gc.x_bearing(), bearings: st.bearings, radius_m: st.radius_km * 1000.0, step_m: 30.0 };
                let (s, n, r) = (st.site.trim().to_string(), st.bearings, st.radius_km);
                crate::settings::set(|x| {
                    x.site = s;
                    x.terrain_bearings = n;
                    x.terrain_radius_km = r;
                });
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(profiles(&req));
                    ctx.request_repaint();
                });
                st.job = Some(rx);
                st.error = None;
            }
            if st.job.is_some() {
                ui.spinner();
                ui.label("Fetching elevation data…");
            }
        });
        if let Some(e) = &st.error {
            ui.colored_label(Color32::from_rgb(190, 60, 30), e);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Tiles are downloaded from Amazon Web Services (which sees roughly where you are looking) and kept in {} for next time. {ATTRIBUTION}",
                cache_dir().map_or("the cache folder".to_string(), |d| d.display().to_string())
            ))
            .small()
            .weak(),
        );
    });
    if !open {
        st.open = false;
    }
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile whose heights follow `f(global px, global py)`.
    fn tile(x: u32, y: u32, f: &dyn Fn(f64, f64) -> f64) -> Vec<u8> {
        let mut img = image::RgbImage::new(256, 256);
        for (i, j, p) in img.enumerate_pixels_mut() {
            let h = f((x * 256 + i) as f64 + 0.5, (y * 256 + j) as f64 + 0.5) + 32768.0;
            let r = (h / 256.0).floor();
            let g = (h - r * 256.0).floor();
            let b = ((h - r * 256.0 - g) * 256.0).round().min(255.0);
            *p = image::Rgb([r as u8, g as u8, b as u8]);
        }
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn tile_coordinates() {
        assert_eq!(tile_xy(0.0, 0.0, 1), (1.0, 1.0));
        let (x, y) = tile_xy(53.0685, -4.0763, 12);
        assert_eq!((x as u32, y as u32), (2001, 1332));
    }

    #[test]
    fn heights_round_trip_through_a_tile() {
        let png = tile(0, 0, &|x, y| 100.0 + x - 0.5 * y + 0.25);
        let h = decode(&png).unwrap();
        assert!((h[0] as f64 - (100.0 + 0.5 - 0.25 + 0.25)).abs() < 0.01, "{}", h[0]);
        assert!((h[255 * 256 + 3] as f64 - (100.0 + 3.5 - 0.5 * 255.5 + 0.25)).abs() < 0.01);
    }

    /// A cone-shaped hill to the north-east of the site, and the profiles across it.
    #[test]
    fn profiles_follow_the_ground() {
        let req = Request { site: (50.0, 10.0), x_bearing: 45.0, bearings: 8, radius_m: 3000.0, step_m: 30.0 };
        // the hill's top, 1.5 km along +X (compass 45°)
        let top = destination(req.site, 45.0, 1500.0 / (EARTH_KM * 1000.0));
        let (tx, ty) = tile_xy(top.0, top.1, ZOOM);
        let (tx, ty) = (tx * 256.0, ty * 256.0);
        let px_m = 2.0 * std::f64::consts::PI * EARTH_KM * 1000.0 * 50f64.to_radians().cos() / (256u64 << ZOOM) as f64;
        let hill = move |x: f64, y: f64| 200.0 + (150.0 - ((x - tx).hypot(y - ty) * px_m) * 0.2).max(0.0);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let fetch = |z: u32, x: u32, y: u32| {
            assert_eq!(z, ZOOM);
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(tile(x, y, &hill))
        };
        let dir = std::env::temp_dir().join(format!("wireant-dem-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = profiles_with(&req, Some(dir.clone()), &fetch).unwrap();
        assert_eq!(f.profiles.len(), 8);
        assert!((f.site_height - 200.0).abs() < 0.5, "{}", f.site_height);
        assert_eq!(f.downloaded, f.tiles);
        let along = &f.profiles[0];
        assert_eq!(along.bearing_deg, 0.0);
        assert_eq!(along.points[0], [0.0, 0.0]);
        // 150 m high at 1.5 km (less the curvature, 0.13 m), level again past 2250 m
        let at = |d: f64| {
            let p = &along.points;
            let i = p.iter().position(|q| q[0] >= d).unwrap();
            let (a, b) = (p[i - 1], p[i]);
            a[1] + (b[1] - a[1]) * (d - a[0]) / (b[0] - a[0])
        };
        assert!((at(1500.0) - 150.0).abs() < 3.0, "{}", at(1500.0));
        assert!((at(1000.0) - 50.0).abs() < 1.5, "{}", at(1000.0));
        assert!(at(2900.0).abs() < 1.5);
        // a cone's sides are straight: thinned to a handful of points
        assert!(along.points.len() < 15, "{:?}", along.points);
        // the opposite way (compass 225°) is level, falling only with the curvature
        let back = &f.profiles[4];
        assert_eq!(back.bearing_deg, 180.0);
        let end = back.points.last().unwrap();
        assert_eq!(end[0], 3000.0);
        assert!((end[1] + 3000.0f64.powi(2) / (2.0 * K_FACTOR * EARTH_KM * 1000.0)).abs() < 0.2, "{end:?}");
        // second time round it all comes from the cache
        let before = calls.load(std::sync::atomic::Ordering::Relaxed);
        let g = profiles_with(&req, Some(dir.clone()), &fetch).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), before);
        assert_eq!(g.downloaded, 0);
        assert_eq!(g.profiles, f.profiles);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_download_is_reported() {
        let req = Request { site: (50.0, 10.0), ..Request::default() };
        let e = profiles_with(&req, None, &|_, _, _| Err("offline".into())).unwrap_err();
        assert_eq!(e, "offline");
        assert!(profiles_with(&Request { site: (89.0, 0.0), ..req.clone() }, None, &|_, _, _| Err("x".into())).is_err());
    }

    #[test]
    fn thinning_keeps_the_corners() {
        let p: Vec<[f64; 2]> = (0..=100).map(|i| [i as f64 * 10.0, if i < 50 { 0.0 } else { (i - 50) as f64 }]).collect();
        assert_eq!(thin(&p, 0.1), vec![[0.0, 0.0], [500.0, 0.0], [1000.0, 50.0]]);
    }

    #[test]
    fn sites() {
        assert_eq!(parse_site("53.0685, -4.0763"), Some((53.0685, -4.0763)));
        assert_eq!(parse_site("53.0685N 4.0763W"), Some((53.0685, -4.0763)));
        assert_eq!(parse_site(" 40.5°S, 120E "), Some((-40.5, 120.0)));
        assert_eq!(parse_site("91, 0"), None);
        assert_eq!(parse_site("IO91"), None); // too coarse to be useful
        let (la, lo) = parse_site("IO91wm").unwrap();
        assert!((la - 51.5208).abs() < 1e-3 && (lo - (-0.125)).abs() < 1e-3);
        let (la8, lo8) = parse_site("IO91wm23").unwrap();
        let (la10, lo10) = parse_site("IO91wm23aa").unwrap();
        // the first 10-character subsquare sits in the 8-character square's south-west corner
        assert!(la10 < la8 && lo10 < lo8);
        assert!((la8 - la10 - (1.0 / 480.0 - 1.0 / 240.0 / 48.0)).abs() < 1e-9);
        assert!((lo8 - lo10 - (1.0 / 240.0 - 2.0 / 240.0 / 48.0)).abs() < 1e-9);
        assert_eq!(parse_site("IO91wm23zz"), None);
    }

    /// Against the real data (needs the network): Snowdon's summit from Llanberis.
    #[test]
    #[ignore]
    fn snowdon_from_llanberis() {
        // Llanberis (about 110 m) to the summit (1085 m; the data has about 1040),
        // 6.3 km at about 168°
        let site = (53.1177, -4.1302);
        let (brg, d) = crate::greatcircle::bearing_distance(site, (53.0685, -4.0763));
        let req = Request { site, x_bearing: brg, bearings: 4, radius_m: 8000.0, step_m: 30.0 };
        let f = profiles_with(&req, None, &download).unwrap();
        let top = f.profiles[0].points.iter().fold(f64::MIN, |m, q| m.max(q[1]));
        println!("site {:.0} m, peak {:.0} m above it, {} km away, {} points", f.site_height, top, d * EARTH_KM, f.profiles[0].points.len());
        assert!((60.0..200.0).contains(&f.site_height));
        assert!((850.0..1000.0).contains(&top));
    }
}
