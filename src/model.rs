//! Antenna description: wires, sources, loads, ground and options.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wire {
    pub p1: [f64; 3],
    pub p2: [f64; 3],
    /// Wire radius in metres.
    pub r: f64,
    /// Segment count; 0 or negative means automatic segmentation.
    pub seg: i32,
    /// Insulation thickness in metres (0 = bare wire).
    pub ins_t: f64,
    /// Relative permittivity of the insulation (PVC about 3.5, PE 2.3, PTFE 2.1).
    pub ins_eps: f64,
}

/// Common insulation materials and their relative permittivity.
pub const INSULATIONS: [(&str, f64); 5] = [("PVC", 3.5), ("Polyethylene", 2.3), ("PTFE (Teflon)", 2.1), ("Rubber", 3.0), ("Enamel", 3.6)];

impl Wire {
    pub fn new(p1: [f64; 3], p2: [f64; 3], r: f64) -> Self {
        Wire { p1, p2, r, seg: -1, ins_t: 0.0, ins_eps: 1.0 }
    }
    pub fn insulated(&self) -> bool {
        self.ins_t > 0.0 && self.ins_eps > 1.0 && self.r.abs() > 0.0
    }
    /// Extra series inductance per metre that a dielectric sleeve adds (H/m):
    /// L' = μ0/(2π) · (1 − 1/εr) · ln((a + t)/a)  — the standard thin-insulation model
    /// (Tsai; also what NEC-4's IS card and common "insulated wire" corrections use).
    pub fn insulation_l_per_m(&self) -> f64 {
        if !self.insulated() {
            return 0.0;
        }
        let a = self.r.abs();
        2.0e-7 * (1.0 - 1.0 / self.ins_eps) * ((a + self.ins_t) / a).ln()
    }
    /// Rough velocity factor of this wire as a resonant element, VF = 1/sqrt(1 + L'/L0),
    /// with L0 = μ0/(2π)·ln(L/a) the wire's own inductance per metre (an estimate for display).
    pub fn velocity_factor(&self) -> f64 {
        let lp = self.insulation_l_per_m();
        if lp <= 0.0 {
            return 1.0;
        }
        let l0 = 2.0e-7 * (self.length().max(10.0 * self.r.abs()) / self.r.abs()).ln().max(1.0);
        1.0 / (1.0 + lp / l0).sqrt()
    }
    pub fn length(&self) -> f64 {
        let d = [self.p2[0] - self.p1[0], self.p2[1] - self.p1[1], self.p2[2] - self.p1[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub wire: usize,
    /// Position along the wire, 0 = begin, 0.5 = centre, 1 = end.
    pub pos: f64,
    pub phase_deg: f64,
    /// Volts, or amps for a current source.
    pub volts: f64,
    /// A current source: it keeps its current (amps, at this phase) whatever the
    /// impedance, as phased arrays are usually fed. Only matters with several sources.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LoadKind {
    /// Fixed impedance R + jX (ohms).
    Rx { r: f64, x: f64 },
    /// Inductor (uH) and/or capacitor (pF) with quality factor Q (0 = lossless).
    /// Only L or only C: a series reactance; both: a parallel L-C trap (Q applies to L).
    /// (FFSolver 0x408200 / 0x407460, see MECHANICS.md §3)
    Lc { l_uh: f64, c_pf: f64, q: f64 },
    /// NEC-style lumped R, L, C in series (LD 0) or parallel (LD 1); a zero value means that
    /// part is absent.
    Rlc { r: f64, l_uh: f64, c_pf: f64, parallel: bool },
    /// A choke of coaxial cable wound into a single-layer air coil, on the wire that stands
    /// for the outside of the coax shield: it blocks common-mode current there.  Modelled as
    /// the coil's inductance in parallel with its self-capacitance, as a trap (see [`Choke`]).
    Choke(Choke),
}

/// An air-wound coax choke ("ugly balun"): `turns` of coax with outer diameter `coax_d`
/// on a former of diameter `form_d`, over `length` (0 = turns touching).  Lengths in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choke {
    pub turns: f64,
    pub form_d: f64,
    pub coax_d: f64,
    /// Winding length (m); 0 = close-wound, turns × coax diameter.
    pub length: f64,
    /// Quality factor of the coil (shield and proximity losses); 0 = lossless.
    pub q: f64,
}

/// Common coax cables and their outer diameter (m).
pub const COAX: [(&str, f64); 5] = [("RG-174", 2.8e-3), ("RG-58", 4.95e-3), ("RG-8X", 6.1e-3), ("RG-213 / RG-8", 10.3e-3), ("LMR-400", 10.29e-3)];

impl Default for Choke {
    /// 8 turns of RG-58 size coax on a 10 cm former.
    fn default() -> Self {
        Choke { turns: 8.0, form_d: 0.1, coax_d: 0.005, length: 0.0, q: 20.0 }
    }
}

impl Choke {
    /// The winding's length (m).
    pub fn winding_length(&self) -> f64 {
        if self.length > 0.0 { self.length } else { self.turns.max(0.0) * self.coax_d.abs() }
    }

    /// The coil's diameter to the centre of the coax (m).
    pub fn coil_d(&self) -> f64 {
        self.form_d.abs() + self.coax_d.abs()
    }

    /// Inductance (µH): a current sheet with Nagaoka's coefficient, good for short and
    /// long coils alike.
    pub fn l_uh(&self) -> f64 {
        let (d, l, n) = (self.coil_d(), self.winding_length(), self.turns.max(0.0));
        if !(d > 0.0 && l > 0.0) {
            return 0.0;
        }
        let a = std::f64::consts::PI * d * d / 4.0;
        4e-7 * std::f64::consts::PI * n * n * a / l * nagaoka(d / l) * 1e6
    }

    /// Self-capacitance (pF), Medhurst's formula for single-layer coils.
    pub fn c_pf(&self) -> f64 {
        let (d, l) = (self.coil_d(), self.winding_length());
        if !(d > 0.0 && l > 0.0) || self.turns <= 0.0 {
            return 0.0;
        }
        let r = l / d;
        (0.1126 * r + 0.08 + 0.27 / r.sqrt()) * d * 100.0
    }

    /// The trap it is modelled as: L, C and Q.
    pub fn as_lc(&self) -> LoadKind {
        LoadKind::Lc { l_uh: self.l_uh(), c_pf: self.c_pf(), q: self.q }
    }

    /// "coax choke 8 t on 100 mm former (9.1 µH ∥ 5.6 pF, resonant 22.3 MHz, Q 20)".
    pub fn describe(&self) -> String {
        let s = short;
        format!(
            "coax choke {} t on {} mm former ({} µH ∥ {} pF, resonant {:.1} MHz, Q {})",
            s(self.turns),
            s(self.form_d * 1000.0),
            s(self.l_uh()),
            s(self.c_pf()),
            self.resonance_mhz(),
            s(self.q)
        )
    }

    /// Self-resonant frequency (MHz), where it blocks best.
    pub fn resonance_mhz(&self) -> f64 {
        let (l, c) = (self.l_uh(), self.c_pf());
        if l > 0.0 && c > 0.0 { 1.0 / (2.0 * std::f64::consts::PI * (l * c).sqrt()) * 1e3 } else { 0.0 }
    }
}

/// A number in at most four significant figures, without trailing zeros ("49", "2.25").
pub fn short(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let d = (3 - v.abs().log10().floor() as i32).clamp(0, 9) as usize;
    let s = format!("{:.*}", d, v);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Nagaoka's coefficient for a solenoid of diameter/length ratio `dl`, from the complete
/// elliptic integrals (k² = d²/(d² + l²)).
fn nagaoka(dl: f64) -> f64 {
    let k2 = dl * dl / (1.0 + dl * dl);
    let k = k2.sqrt();
    let kp = (1.0 - k2).sqrt();
    // complete elliptic integrals by the arithmetic-geometric mean
    let (mut a, mut b, mut sum, mut p) = (1.0f64, kp, k2 / 2.0, 0.5);
    for _ in 0..40 {
        let an = (a + b) / 2.0;
        let c = (a - b) / 2.0;
        b = (a * b).sqrt();
        a = an;
        p *= 2.0;
        sum += p * c * c;
        if c.abs() < 1e-16 {
            break;
        }
    }
    let kk = std::f64::consts::PI / (2.0 * a);
    let ee = kk * (1.0 - sum);
    4.0 / (3.0 * std::f64::consts::PI * kp) * ((kp * kp / k2) * (kk - ee) + ee - k)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Load {
    pub wire: usize,
    pub pos: f64,
    pub kind: LoadKind,
}

impl Load {
    pub fn impedance(&self, freq_hz: f64) -> num_complex::Complex64 {
        self.kind.impedance(freq_hz)
    }
}

impl LoadKind {
    /// A series inductor (X > 0) or capacitor (X < 0) with reactance `x` ohms at `freq_hz`.
    pub fn reactance(x: f64, freq_hz: f64) -> LoadKind {
        let w = 2.0 * std::f64::consts::PI * freq_hz;
        if x >= 0.0 {
            LoadKind::Lc { l_uh: x / w * 1e6, c_pf: 0.0, q: 0.0 }
        } else {
            LoadKind::Lc { l_uh: 0.0, c_pf: -1e12 / (w * x), q: 0.0 }
        }
    }

    /// The kind as one the file formats know: a choke becomes its L-C trap.
    pub fn plain(&self) -> LoadKind {
        match self {
            LoadKind::Choke(c) => c.as_lc(),
            k => k.clone(),
        }
    }

    pub fn impedance(&self, freq_hz: f64) -> num_complex::Complex64 {
        use num_complex::Complex64 as C;
        // The original works with f in MHz, L in uH and C in pF (FFSolver 0x408200 / 0x407460).
        const TWO_PI: f64 = 6.283185307179586; // DAT_00425b10
        const TWO_PI_E6: f64 = 6.283185307179586e-06; // DAT_00425b08
        let f_mhz = freq_hz * 1e-6;
        match *self {
            LoadKind::Choke(c) => c.as_lc().impedance(freq_hz),
            LoadKind::Rx { r, x } => C::new(r, x),
            LoadKind::Lc { l_uh, c_pf, q } => {
                let xl = f_mhz * l_uh * TWO_PI;
                if l_uh != 0.0 && c_pf != 0.0 {
                    let rl = if q != 0.0 { xl / q } else { 0.0 };
                    let y = 1.0 / C::new(rl, xl) + C::new(0.0, f_mhz * c_pf * TWO_PI_E6);
                    if y.norm() == 0.0 { C::new(1e20, 0.0) } else { 1.0 / y }
                } else if l_uh != 0.0 {
                    C::new(if q != 0.0 { xl / q } else { 0.0 }, xl)
                } else if c_pf != 0.0 {
                    let xc = -1.0 / (f_mhz * c_pf * TWO_PI_E6);
                    C::new(if q != 0.0 { -xc / q } else { 0.0 }, xc)
                } else {
                    C::new(0.0, 0.0)
                }
            }
            LoadKind::Rlc { r, l_uh, c_pf, parallel } => {
                let w = TWO_PI * freq_hz;
                if parallel {
                    let mut y = C::new(0.0, 0.0);
                    if r != 0.0 {
                        y += 1.0 / r;
                    }
                    if l_uh != 0.0 {
                        y += 1.0 / C::new(0.0, w * l_uh * 1e-6);
                    }
                    if c_pf != 0.0 {
                        y += C::new(0.0, w * c_pf * 1e-12);
                    }
                    if y.norm() == 0.0 { C::new(1e20, 0.0) } else { 1.0 / y }
                } else {
                    let mut z = C::new(r, 0.0);
                    if l_uh != 0.0 {
                        z += C::new(0.0, w * l_uh * 1e-6);
                    }
                    if c_pf != 0.0 {
                        z += C::new(0.0, -1.0 / (w * c_pf * 1e-12));
                    }
                    z
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroundKind {
    Free,
    Perfect,
    Real,
}

impl GroundKind {
    pub fn label(&self) -> &'static str {
        match self {
            GroundKind::Free => "Free space",
            GroundKind::Perfect => "Perfect ground",
            GroundKind::Real => "Real ground",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ground {
    pub kind: GroundKind,
    /// Relative permittivity of real ground.
    pub eps_r: f64,
    /// Conductivity of real ground in mS/m.
    pub sigma_ms: f64,
    /// Extra height added to every wire when a ground is used (m).
    pub add_height: f64,
    /// A second ground medium (sea, marsh) away from the antenna, far field only.
    pub second: SecondMedium,
}

/// A second ground medium beyond a straight boundary or outside a circle, as
/// NEC-2's GD card: it changes the ground reflection in the far field, not the
/// currents.  Positions are relative to the origin (before "add height").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecondMedium {
    pub enabled: bool,
    pub eps_r: f64,
    /// mS/m
    pub sigma_ms: f64,
    /// boundary is a circle about the origin (medium 2 outside) instead of a line
    pub circle: bool,
    /// distance from the origin to the line, or the circle's radius (m)
    pub distance: f64,
    /// direction from the origin square to the line, degrees from +X toward +Y
    /// (azimuth, as the patterns): medium 2 lies that way
    pub bearing_deg: f64,
    /// how far medium 2's surface lies below the primary ground (m)
    pub drop: f64,
    /// smooth transition over the first Fresnel zone (wiremom, MININEC) instead of
    /// the medium at the reflection point (NEC-2)
    pub fresnel: bool,
}

impl Default for SecondMedium {
    fn default() -> Self {
        SecondMedium { enabled: false, eps_r: 81.0, sigma_ms: 5000.0, circle: false, distance: 30.0, bearing_deg: 0.0, drop: 0.0, fresnel: true }
    }
}

impl SecondMedium {
    /// The wiremom description (None unless enabled).
    pub fn to_wiremom(&self) -> Option<wiremom::SecondGround> {
        if !self.enabled {
            return None;
        }
        let boundary = if self.circle {
            wiremom::Boundary::Circle { radius: self.distance.max(0.0) }
        } else {
            wiremom::Boundary::Line { distance: self.distance, azimuth: self.bearing_deg.to_radians() }
        };
        let mut g = wiremom::SecondGround::new(wiremom::RealGround { eps_r: self.eps_r, sigma: self.sigma_ms * 1e-3 }, boundary);
        g.drop = self.drop.max(0.0);
        g.fresnel = self.fresnel;
        Some(g)
    }

    /// One-line form stored in MMANA comments and NEC CM cards.
    pub fn encode(&self) -> String {
        format!(
            "wireant ground2 {} {} {} {} {} {} {}",
            self.eps_r,
            self.sigma_ms,
            if self.circle { "circle" } else { "line" },
            self.distance,
            self.bearing_deg,
            self.drop,
            if self.fresnel { "fresnel" } else { "specular" }
        )
    }

    pub fn decode(line: &str) -> Option<SecondMedium> {
        let rest = line.trim().strip_prefix("wireant ground2")?;
        let t: Vec<&str> = rest.split_whitespace().collect();
        if t.len() < 7 {
            return None;
        }
        let f = |i: usize| t[i].parse::<f64>().ok();
        Some(SecondMedium {
            enabled: true,
            eps_r: f(0)?,
            sigma_ms: f(1)?,
            circle: t[2] == "circle",
            distance: f(3)?,
            bearing_deg: f(4)?,
            drop: f(5)?,
            fresnel: t[6] != "specular",
        })
    }
}

/// Typical ground constants (εr, σ in mS/m), as in NEC/ITU tables.
pub const GROUND_PRESETS: [(&str, f64, f64); 7] = [
    ("Sea water", 81.0, 5000.0),
    ("Fresh water", 80.0, 1.0),
    ("Very good (pastoral, rich soil)", 20.0, 30.0),
    ("Average", 13.0, 5.0),
    ("Poor (rocky, sandy)", 5.0, 1.0),
    ("Very poor (city, industrial)", 3.0, 1.0),
    ("Wet ground (ITU)", 30.0, 10.0),
];

/// Which solver calculates the antenna.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// The built-in multi-threaded MININEC-style solver.
    Mininec,
    /// The bundled public-domain NEC-2 engine (nec2c), with Sommerfeld real ground.
    Nec2,
    /// The wiremom engine: MPIE with triangle basis functions and Galerkin testing,
    /// Sommerfeld real ground, in-process and multi-threaded.
    Wiremom,
    /// An external NEC program the user has a licence for (NEC-4, NEC-5 or another
    /// build), set under File → Preferences; run like NEC-2 with the same card deck.
    External,
}

impl Engine {
    pub fn label(&self) -> &'static str {
        match self {
            Engine::Mininec => "MININEC (built-in, multi-threaded)",
            Engine::Nec2 => "NEC-2 (nec2c, Sommerfeld ground)",
            Engine::Wiremom => "wiremom (built-in MPIE, Sommerfeld ground)",
            Engine::External => "External NEC (your own copy; experimental)",
        }
    }
}

/// Wire materials in the original's order; the index is the code in the .maa file
/// (language file english.mmn: WireSel0..WireSel8). Conductivity and permeability values are
/// estimates (the protected main program holds the real ones); "pipe" uses the same skin-effect
/// loss as "wire".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Lossless,
    CuWire,
    CuPipe,
    AlWire,
    AlPipe,
    FeWire,
    FePipe,
    UserWire,
    UserPipe,
}

impl Material {
    pub const ALL: [Material; 9] = [
        Material::Lossless,
        Material::CuWire,
        Material::CuPipe,
        Material::AlWire,
        Material::AlPipe,
        Material::FeWire,
        Material::FePipe,
        Material::UserWire,
        Material::UserPipe,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            Material::Lossless => "No loss",
            Material::CuWire => "Cu wire",
            Material::CuPipe => "Cu pipe",
            Material::AlWire => "Al wire",
            Material::AlPipe => "Al pipe",
            Material::FeWire => "Fe wire",
            Material::FePipe => "Fe pipe",
            Material::UserWire => "User wire",
            Material::UserPipe => "User pipe",
        }
    }
    /// (conductivity S/m, relative permeability); None = perfect conductor.
    pub fn conduction(&self, user_rho: f64, user_mu: f64) -> Option<(f64, f64)> {
        match self {
            Material::Lossless => None,
            Material::CuWire | Material::CuPipe => Some((5.8e7, 1.0)),
            Material::AlWire | Material::AlPipe => Some((3.5e7, 1.0)),
            Material::FeWire | Material::FePipe => Some((1.0e7, 100.0)),
            Material::UserWire | Material::UserPipe => {
                if user_rho > 0.0 { Some((1.0 / user_rho, user_mu.max(1.0))) } else { None }
            }
        }
    }
    pub fn is_user(&self) -> bool {
        matches!(self, Material::UserWire | Material::UserPipe)
    }
    /// A tube: its free ends are open (no end cap), where a wire is a solid rod with flat
    /// ends. Choosing a material sets [`Antenna::open_ends`] to match.
    pub fn is_pipe(&self) -> bool {
        matches!(self, Material::CuPipe | Material::AlPipe | Material::FePipe | Material::UserPipe)
    }
    pub fn code(&self) -> i32 {
        Material::ALL.iter().position(|m| m == self).unwrap_or(0) as i32
    }
    pub fn from_code(c: i32) -> Material {
        *Material::ALL.get(c.max(0) as usize).unwrap_or(&Material::Lossless)
    }
}

/// Automatic segmentation: shortest segment lambda/dm1 at the wire ends, longest lambda/dm2,
/// growing by factor `sc` from the ends; `ec` = number of shortest segments at each end
/// (FFSolver 0x405540, see MECHANICS.md §2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Segmentation {
    pub dm1: f64,
    pub dm2: f64,
    pub sc: f64,
    pub ec: i32,
}

impl Default for Segmentation {
    fn default() -> Self {
        Segmentation { dm1: 400.0, dm2: 40.0, sc: 2.0, ec: 1 }
    }
}

/// A flat conducting plate (parallelogram) from `corner` along `edge1` and
/// `edge2`.  Modelled by the wiremom engine; a wire ending on the plate connects to
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plate {
    pub corner: [f64; 3],
    pub edge1: [f64; 3],
    pub edge2: [f64; 3],
    /// Mesh divisions along each edge; 0 = automatic (about a tenth of a wavelength).
    pub n1: usize,
    pub n2: usize,
}

impl Plate {
    pub fn new(corner: [f64; 3], edge1: [f64; 3], edge2: [f64; 3]) -> Plate {
        Plate { corner, edge1, edge2, n1: 0, n2: 0 }
    }
    pub fn corners(&self) -> [[f64; 3]; 4] {
        let p = |a: f64, b: f64| [0, 1, 2].map(|i| self.corner[i] + a * self.edge1[i] + b * self.edge2[i]);
        [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)]
    }
}

/// A wound transformer (balun or unun) between two places on wires, as a two-port
/// network: end a is the primary, where the feed comes in, end b the secondary.  The
/// windings are coupled inductors on a core: a primary inductance `l_uh` (the
/// magnetising inductance, from the core's AL and the primary turns), the secondary
/// `ratio` times that, coupled by `k`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transformer {
    /// Impedance ratio N of an N:1 transformer: the impedance at end b is N times that at
    /// end a (turns ratio √N).  49 for an end-fed half wave, 9 or 4 for random wires.
    pub ratio: f64,
    /// Primary inductance (µH); 0 = ideal (no magnetising current).
    pub l_uh: f64,
    /// Coupling between the windings (1 = no leakage inductance).
    pub k: f64,
    /// Q of the windings on the core: core loss (0 = no loss).
    pub q: f64,
}

impl Default for Transformer {
    fn default() -> Self {
        Transformer { ratio: 49.0, l_uh: 0.0, k: 1.0, q: 0.0 }
    }
}

/// Inductance of `turns` on a core with inductance factor `al_nh` (nH per turn²), µH.
pub fn core_l_uh(al_nh: f64, turns: f64) -> f64 {
    al_nh * turns * turns * 1e-3
}

/// Common ferrite toroids: name and AL (nH/turn², the makers' nominal values).
pub const CORES: [(&str, f64); 8] = [
    ("FT240-43", 1075.0),
    ("FT240-52", 1000.0),
    ("FT240-61", 171.0),
    ("FT140-43", 885.0),
    ("FT114-43", 510.0),
    ("FT82-43", 557.0),
    ("FT240-31", 1300.0),
    ("FT240-77 (MF)", 3170.0),
];

impl Transformer {
    /// Short-circuit admittance parameters (y11, y12, y22) at `freq_hz`; `reversed` swaps
    /// the secondary's ends.
    pub fn admittance(&self, freq_hz: f64, reversed: bool) -> (num_complex::Complex64, num_complex::Complex64, num_complex::Complex64) {
        use num_complex::Complex64 as C;
        let w = 2.0 * std::f64::consts::PI * freq_hz;
        // a small winding resistance keeps a tightly coupled (or ideal) transformer's
        // admittances finite
        let r_w = C::new(1e-3, 0.0);
        let n2 = if self.ratio > 0.0 { self.ratio } else { 1.0 };
        // the T equivalent referred to the primary: leakage (1 − k)L1 in series on each
        // side, kL1 across; a lossy core makes each inductance jωL(1 − j/Q)
        let (y11, y12, y22) = if self.l_uh > 0.0 {
            let k = self.k.clamp(0.0, 1.0);
            let jw_l = C::new(0.0, w * self.l_uh * 1e-6) * if self.q > 0.0 { C::new(1.0, -1.0 / self.q) } else { C::new(1.0, 0.0) };
            let (zs, zm) = (jw_l * (1.0 - k) + r_w, jw_l * k);
            let (z11, z12) = (zs + zm, zm);
            let det = z11 * z11 - z12 * z12;
            (z11 / det, -z12 / det, z11 / det)
        } else {
            let y = 1.0 / (2.0 * r_w);
            (y, -y, y)
        };
        let n = n2.sqrt();
        let s = if reversed { -1.0 } else { 1.0 };
        (y11, y12 * (s / n), y22 / n2)
    }

    pub fn label(&self) -> String {
        let r = self.ratio;
        if r >= 1.0 { format!("{}:1", short(r)) } else if r > 0.0 { format!("1:{}", short(1.0 / r)) } else { "?".into() }
    }
}

/// A transmission line between two places on wires (a feed line, phasing line or
/// stub).  Modelled by the wiremom and NEC-2 engines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TLine {
    pub a_wire: usize,
    pub a_pos: f64,
    pub b_wire: usize,
    pub b_pos: f64,
    /// Characteristic impedance (ohms).
    pub z0: f64,
    /// Length in metres; 0 = the straight distance between the two ends.
    pub length: f64,
    /// Velocity factor (1 = air line, about 0.66 for solid-PE coax).
    pub vf: f64,
    /// Loss in dB per 100 m at the calculation frequency.
    pub loss_db: f64,
    /// Conductors swapped at the far end (a half twist; NEC's negative Z0).
    pub reversed: bool,
    /// A lumped part across end a (in parallel with whatever is there), as NEC's TL shunt admittance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shunt_a: Option<LoadKind>,
    /// A lumped part across end b.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shunt_b: Option<LoadKind>,
    /// A lumped part joining the two ends instead of a line (a series L or C in a matching
    /// network): z0, length, vf, loss and reversed are then unused.  NEC's NT card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<LoadKind>,
    /// A transformer joining the two ends instead of a line: end a is its primary.
    /// `reversed` swaps the secondary's ends; the shunt parts still apply (a compensating
    /// capacitor across the primary, say).  NEC's NT card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transformer: Option<Transformer>,
}

impl TLine {
    pub fn new(a_wire: usize, a_pos: f64, b_wire: usize, b_pos: f64) -> TLine {
        TLine { a_wire, a_pos, b_wire, b_pos, z0: 50.0, length: 0.0, vf: 1.0, loss_db: 0.0, reversed: false, shunt_a: None, shunt_b: None, series: None, transformer: None }
    }

    /// The lumped parts' admittances at `freq_hz`: the series part's (None for a line),
    /// and those across end a and end b (0 when there is none).
    pub fn part_admittances(&self, freq_hz: f64) -> (Option<num_complex::Complex64>, num_complex::Complex64, num_complex::Complex64) {
        let y = |k: &Option<LoadKind>| k.as_ref().map(|k| 1.0 / k.impedance(freq_hz));
        let zero = num_complex::Complex64::new(0.0, 0.0);
        (y(&self.series), y(&self.shunt_a).unwrap_or(zero), y(&self.shunt_b).unwrap_or(zero))
    }

    /// The admittance parameters (y11, y12, y22) of a lumped network (a series part or a
    /// transformer, with any shunt parts); None for a transmission line.
    pub fn network(&self, freq_hz: f64) -> Option<(num_complex::Complex64, num_complex::Complex64, num_complex::Complex64)> {
        let (ys, ya, yb) = self.part_admittances(freq_hz);
        if let Some(x) = &self.transformer {
            let (y11, y12, y22) = x.admittance(freq_hz, self.reversed);
            return Some((y11 + ya, y12, y22 + yb));
        }
        ys.map(|y| (y + ya, -y, y + yb))
    }

    /// A transformer from end a to end b with its primary inductance and a compensating
    /// capacitor (pF, 0 = none) across the primary.
    pub fn transformer(a_wire: usize, a_pos: f64, b_wire: usize, b_pos: f64, x: Transformer, c_pf: f64) -> TLine {
        let mut t = TLine::new(a_wire, a_pos, b_wire, b_pos);
        t.transformer = Some(x);
        t.shunt_a = (c_pf > 0.0).then_some(LoadKind::Lc { l_uh: 0.0, c_pf, q: 0.0 });
        t
    }

    pub fn has_parts(&self) -> bool {
        self.shunt_a.is_some() || self.shunt_b.is_some() || self.series.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Antenna {
    pub name: String,
    pub freq_mhz: f64,
    pub wires: Vec<Wire>,
    pub sources: Vec<Source>,
    pub loads: Vec<Load>,
    /// Transmission lines (not stored in .maa files).
    pub lines: Vec<TLine>,
    /// Conducting plates (not stored in .maa files).
    pub plates: Vec<Plate>,
    pub ground: Ground,
    pub material: Material,
    /// Reference impedance for SWR (ohms).
    pub z0: f64,
    pub seg: Segmentation,
    pub comment: String,
    /// Rear range for F/B (degrees): azimuth width around the back and vertical range
    /// (the "AzEl" fields of the G/H/M/R/AzEl/X line, 120 and 60 in most example files).
    pub fb_az: f64,
    pub fb_el: f64,
    /// User material: resistivity (ohm·m) and relative permeability.
    pub user_rho: f64,
    pub user_mu: f64,
    /// Solver choice (an app setting, not stored in .maa files).
    pub engine: Engine,
    /// Wire ends are open, as on a hollow tube, rather than flat caps that carry charge
    /// like the end of a solid rod (wiremom only: MININEC and NEC treat every end as open).
    pub open_ends: bool,
}

pub fn pos_label(wire: usize, pos: f64) -> String {
    if pos <= 1e-9 {
        format!("w{}b", wire + 1)
    } else if (pos - 0.5).abs() < 1e-9 {
        format!("w{}c", wire + 1)
    } else if pos >= 1.0 - 1e-9 {
        format!("w{}e", wire + 1)
    } else {
        format!("w{}p{:.1}", wire + 1, pos * 100.0)
    }
}

impl Antenna {
    pub fn lambda(&self) -> f64 {
        299.792458 / self.freq_mhz
    }

    pub fn example_names() -> &'static [&'static str] {
        &[
            "Half-wave dipole (14 MHz)",
            "Inverted V (7 MHz)",
            "3-element Yagi (14 MHz)",
            "Quarter-wave vertical (7 MHz)",
            "Ground plane with 4 radials (28 MHz)",
            "Two-element quad (21 MHz)",
            "End-fed half wave, 49:1 unun and coax choke (7 MHz)",
        ]
    }

    /// A new, empty antenna: no wires, sources or loads; free space, copper wire,
    /// 14.15 MHz until the user sets the frequency.
    pub fn blank() -> Antenna {
        let mut a = Antenna::example(0);
        a.name = "New antenna".into();
        a.wires.clear();
        a.sources.clear();
        a.loads.clear();
        a.lines.clear();
        a.plates.clear();
        a.comment.clear();
        a.ground = Ground { kind: GroundKind::Free, eps_r: 13.0, sigma_ms: 5.0, add_height: 0.0, second: SecondMedium::default() };
        a.material = Material::CuWire;
        a
    }

    /// A built-in example, its wire ends matching its material (open for pipe).
    pub fn example(i: usize) -> Antenna {
        let mut a = Antenna::example_wires(i);
        a.open_ends = a.material.is_pipe();
        a
    }

    fn example_wires(i: usize) -> Antenna {
        let real = |h: f64| Ground { kind: GroundKind::Real, eps_r: 13.0, sigma_ms: 5.0, add_height: h, second: SecondMedium::default() };
        let base = |name: &str, f: f64| Antenna {
            name: name.to_string(),
            freq_mhz: f,
            wires: vec![],
            sources: vec![],
            loads: vec![],
            lines: vec![],
            plates: vec![],
            ground: Ground { kind: GroundKind::Free, eps_r: 13.0, sigma_ms: 5.0, add_height: 0.0, second: SecondMedium::default() },
            material: Material::CuWire,
            z0: 50.0,
            seg: Segmentation::default(),
            comment: String::new(),
            fb_az: 120.0,
            fb_el: 60.0,
            user_rho: 1.72e-8,
            user_mu: 1.0,
            engine: Engine::Wiremom,
            open_ends: false,
        };
        let src = |w| Source { wire: w, pos: 0.5, phase_deg: 0.0, volts: 1.0, current: false };
        match i {
            1 => {
                let mut a = base("Inverted V for 7.1 MHz", 7.1);
                a.wires.push(Wire::new([0.0, 0.0, 12.0], [0.0, -9.0, 7.5], 1.0e-3));
                a.wires.push(Wire::new([0.0, 0.0, 12.0], [0.0, 9.0, 7.5], 1.0e-3));
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0, current: false });
                a.ground = real(0.0);
                a
            }
            2 => {
                let mut a = base("3-element Yagi for 14.15 MHz", 14.15);
                let r = 12.5e-3;
                a.wires.push(Wire::new([-4.2, -5.42, 0.0], [-4.2, 5.42, 0.0], r));
                a.wires.push(Wire::new([0.0, -5.08, 0.0], [0.0, 5.08, 0.0], r));
                a.wires.push(Wire::new([3.2, -4.86, 0.0], [3.2, 4.86, 0.0], r));
                a.sources.push(src(1));
                a.ground = real(15.0);
                a.material = Material::AlPipe;
                a
            }
            3 => {
                let mut a = base("Quarter-wave vertical for 7.1 MHz", 7.1);
                a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, 10.2], 5.0e-3));
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0, current: false });
                a.ground = Ground { kind: GroundKind::Perfect, ..real(0.0) };
                a
            }
            4 => {
                let mut a = base("Ground plane for 28.5 MHz", 28.5);
                let r = 2.0e-3;
                a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, 2.55], r));
                let d = 2.6 * std::f64::consts::FRAC_1_SQRT_2;
                let drop = -1.2;
                for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                    let _ = d;
                    a.wires.push(Wire::new([0.0, 0.0, 0.0], [x * 2.35, y * 2.35, drop], r));
                }
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0, current: false });
                a
            }
            5 => {
                let mut a = base("Two-element quad for 21.2 MHz", 21.2);
                let r = 1.0e-3;
                let sq = |x: f64, s: f64, a: &mut Antenna| {
                    let p = [[x, -s, -s], [x, s, -s], [x, s, s], [x, -s, s]];
                    for k in 0..4 {
                        a.wires.push(Wire::new(p[k], p[(k + 1) % 4], r));
                    }
                };
                sq(0.0, 1.80, &mut a);
                sq(-2.8, 1.88, &mut a);
                a.sources.push(src(0));
                a
            }
            6 => {
                const RADIATOR: f64 = 20.0;
                let mut a = base("End-fed half wave for 7.1 MHz", 7.1);
                a.ground = real(0.0);
                // the radiator, the unun's terminals (a 20 cm gap) and the coax's shield
                // running down to the station ground, which acts as the counterpoise
                a.wires.push(Wire::new([0.0, 0.0, 10.0], [RADIATOR, 0.0, 10.0], 1.0e-3));
                a.wires.push(Wire::new([0.0, 0.0, 10.0], [0.0, 0.0, 9.8], 1.0e-3));
                a.wires[1].seg = 3;
                a.wires.push(Wire::new([0.0, 0.0, 9.8], [0.0, 0.0, 0.0], 2.5e-3));
                a.sources.push(Source { wire: 1, pos: 0.5, phase_deg: 0.0, volts: 1.0, current: false });
                // 49:1 on an FT240-43 with 3 primary turns, fed from a small feed wire
                let x = Transformer { ratio: 49.0, l_uh: core_l_uh(1075.0, 3.0), k: 0.99, q: 0.0 };
                crate::matching::feed_through_transformer(&mut a, 0, x);
                // 8 turns of RG-58 on a 20 cm former, 1 m above the ground
                a.loads.push(Load { wire: 2, pos: 0.9, kind: LoadKind::Choke(Choke { turns: 8.0, form_d: 0.2, coax_d: 0.005, length: 0.0, q: 20.0 }) });
                a.comment = "Wire 3 is the outside of the coax shield, the counterpoise. Delete the choke (or move it up wire 3) to see how much the feed depends on the coax.".into();
                a
            }
            _ => {
                let mut a = base("Half-wave dipole for 14.15 MHz", 14.15);
                a.wires.push(Wire::new([0.0, -5.14, 0.0], [0.0, 5.14, 0.0], 1.0e-3));
                a.sources.push(src(0));
                a
            }
        }
    }
}

// Defaults let JSON (the MCP interface) leave fields out.
impl Default for Wire {
    fn default() -> Self {
        Wire::new([0.0; 3], [0.0; 3], 1.0e-3)
    }
}
impl Default for Source {
    fn default() -> Self {
        Source { wire: 0, pos: 0.5, phase_deg: 0.0, volts: 1.0, current: false }
    }
}
impl Default for Load {
    fn default() -> Self {
        Load { wire: 0, pos: 0.5, kind: LoadKind::Rx { r: 0.0, x: 0.0 } }
    }
}
impl Default for Ground {
    fn default() -> Self {
        Antenna::blank().ground
    }
}
impl Default for Plate {
    fn default() -> Self {
        Plate::new([0.0; 3], [0.0; 3], [0.0; 3])
    }
}
impl Default for TLine {
    fn default() -> Self {
        TLine::new(0, 0.5, 0, 0.5)
    }
}
impl Default for Antenna {
    fn default() -> Self {
        Antenna::blank()
    }
}

#[cfg(test)]
mod wire_end_tests {
    use super::*;

    #[test]
    fn pipes_have_open_ends() {
        assert!(Material::AlPipe.is_pipe() && !Material::AlWire.is_pipe() && !Material::Lossless.is_pipe());
        // the Yagi example is aluminium pipe, the dipole copper wire
        assert!(Antenna::example(2).open_ends);
        assert!(!Antenna::example(0).open_ends);
    }
}

#[cfg(test)]
mod choke_transformer_tests {
    use super::*;
    use num_complex::Complex64 as C;

    #[test]
    fn nagaoka_coefficient() {
        // tabulated values: 0.6884 for a coil as long as it is wide, 0.8181 at l = 2d
        assert!((nagaoka(1.0) - 0.6884).abs() < 1e-3, "{}", nagaoka(1.0));
        assert!((nagaoka(0.5) - 0.8181).abs() < 1e-3, "{}", nagaoka(0.5));
        assert!((nagaoka(1e-3) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn coax_choke_values() {
        // 8 turns of RG-58 on a 10 cm former: about 9 µH and 6 pF, resonant in the low 20s of MHz
        let c = Choke::default();
        println!("{}", c.describe());
        assert!((7.0..12.0).contains(&c.l_uh()), "{}", c.l_uh());
        assert!((4.0..8.0).contains(&c.c_pf()), "{}", c.c_pf());
        // a high impedance at resonance, Q times the reactance
        let f = c.resonance_mhz() * 1e6;
        let z = LoadKind::Choke(c).impedance(f);
        let xl = 2.0 * std::f64::consts::PI * f * c.l_uh() * 1e-6;
        assert!((z.re - c.q * xl).abs() < 0.05 * c.q * xl, "{z} vs {}", c.q * xl);
    }

    /// Input impedance at end a with `zl` on end b.
    fn z_in((y11, y12, y22): (C, C, C), zl: C) -> C {
        1.0 / (y11 - y12 * y12 / (y22 + 1.0 / zl))
    }

    #[test]
    fn ideal_transformer_divides_the_impedance() {
        for n in [4.0, 9.0, 49.0] {
            let x = Transformer { ratio: n, ..Transformer::default() };
            let zl = C::new(2450.0, -300.0);
            let z = z_in(x.admittance(7e6, false), zl);
            assert!((z - zl / n).norm() < 1e-3 * (zl / n).norm(), "{n}: {z}");
            // and back the other way
            assert!((z_in(x.admittance(7e6, true), zl) - zl / n).norm() < 1e-3 * (zl / n).norm());
        }
    }

    #[test]
    fn real_transformer_has_magnetising_and_leakage_effects() {
        // 49:1 on an FT240-43 with 3 primary turns: about 9.7 µH
        let x = Transformer { ratio: 49.0, l_uh: core_l_uh(1075.0, 3.0), k: 0.99, q: 0.0 };
        let zl = C::new(2450.0, 0.0);
        let lo = z_in(x.admittance(3.6e6, false), zl);
        let mid = z_in(x.admittance(14e6, false), zl);
        let hi = z_in(x.admittance(28e6, false), zl);
        println!("3.6 MHz {lo:.2}, 14 MHz {mid:.2}, 28 MHz {hi:.2}");
        // the magnetising inductance shunts the input at low frequencies (inductive),
        // the leakage adds series inductance higher up
        assert!(lo.im > 0.0 && lo.re < 50.0);
        assert!((mid.re - 50.0).abs() < 10.0);
        assert!(hi.im > mid.im);
        // a lossy core loses power: the input resistance includes it
        let lossy = Transformer { q: 2.0, ..x };
        assert!(z_in(lossy.admittance(3.6e6, false), zl).re != lo.re);
    }
}
