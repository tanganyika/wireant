# Ground

Choose the ground on the Geometry or Calculate tab.

- **Free space**: no ground at all. This is good for comparing designs, and for VHF/UHF antennas
  high above the ground.
- **Perfect ground**: a perfectly conducting flat surface at Z = 0. A quarter-wave vertical on it
  behaves like an ideal ground-plane antenna.
- **Real ground**: soil with a relative permittivity (**Ground εr**) and conductivity
  (**Ground σ**, in mS/m). It absorbs some power and changes the pattern, most of all at low
  elevations.

**Presets** fills in typical values:

| Ground | εr | σ (mS/m) |
|---|---|---|
| Sea water | 81 | 5000 |
| Fresh water | 80 | 1 |
| Very good (pastoral, rich soil) | 20 | 30 |
| Average | 13 | 5 |
| Poor (rocky, sandy) | 5 | 1 |
| Very poor (city, industrial) | 3 | 1 |
| Wet ground (ITU) | 30 | 10 |

**Add height (m)** raises the whole antenna by that amount above the ground, without changing
the wire coordinates. This is a quick way to try different heights.

## Sloping ground

Set **Slope** to model an antenna on a hillside. The ground's surface is tilted by that many
degrees through the origin, falling toward the bearing in **falling toward** (an azimuth: 0° = +X,
90° = +Y). Along that bearing the ground is lower by tan(slope) × the distance from the origin.

- The wires keep their coordinates, so a mast stays vertical while the ground tilts under it. Put
  the antenna's base at the origin, where the ground is at Z = 0. Wires touching the ground, radials
  lying on it and buried wires are measured against the sloping surface, so a radial lying on it
  needs its far end at the ground's height there.
- The patterns reach below the horizon downhill: the vertical pattern draws the ground line
  tilted, and the gain is zero below the ground. A lower takeoff angle toward the valley is the
  main effect of a hillside site.
- The slope runs on for ever. That is exact for the impedance and for rays that reflect near the
  antenna, but optimistic for low rays toward the valley, which in reality reflect from ground
  far away.
- Sloping ground needs the **wiremom** engine, and can't yet be combined with a second ground.

## Terrain

Tick **Terrain** to describe the hills, ridges and valleys around the antenna, as terrain-analysis
programs such as HFTA do. The **Terrain** card on the Geometry tab holds one or more **profiles**:
the ground's height along a bearing from the origin, as a list of points (**Distance** and
**Height**, in metres). Heights are relative to the ground at the origin, where the antenna
stands, and beyond the last point the ground stays level. Between two profiles' bearings the
heights are blended; a single profile applies all round. **➕ Profile** adds another bearing,
**➕ Point** another point. Next to each bearing the card shows the terrain's highest angle above
the ground at the origin, a quick guide to which low angles it can block.

**Import…** reads a profile from a text file with a distance and a height on each line, in metres
or feet, as terrain-profile tools export them. Lines that don't start with two numbers (headers,
comments) are skipped, and heights above sea level are made relative to the first point.

### Terrain from elevation data

**🗺 From elevation data…** on the Terrain card fills it with profiles all round the antenna
from map elevation data, as Radio Mobile and HFTA's helper tools use. Give:

- **Antenna at**: the latitude and longitude of the model's origin, in degrees (south and west
  negative, e.g. `53.1177, -4.1302`, or `53.1177N 4.1302W`). Most map sites show them when you
  right-click a place. A 10-character Maidenhead locator also works; a 6-character one is a
  square several km across, far too coarse for the ground near the antenna.
- **+X axis points to**: the compass bearing of the model's +X axis, the same setting as the
  Great circle map's. Profiles are in the model's azimuths, as the patterns are.
- **Profiles** (36, every 10°, by default) and how far out they run (5 km by default). Low rays
  reflect from up to a few km away; hills further out only cut the very lowest angles.

**Fetch terrain** replaces the card's profiles (Undo in the View tab brings the old ones back).
The position and the profile settings are remembered: on any other antenna, ticking **Terrain**
fills the card straight away from the elevation data already downloaded for that position,
without opening this window or using the network. Antennas saved with terrain keep their own.
The heights are relative to the ground at the antenna and fall away with the earth's curvature
(using 4/3 of the earth's radius, for ordinary refraction): about 1.5 m at 5 km. Each profile
keeps only the points needed to stay within 1 m of the data.

The data is the [AWS Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) (Mapzen /
Tilezen), built from SRTM, USGS 3DEP, GMTED2010, ETOPO1, Geoscience Australia, Canada CDEM,
EU-DEM, Kartverket, LINZ and others, on a grid about 25 to 38 m across. Things to know:

- It measures the surface, partly including trees and buildings, and smooths sharp peaks and
  small banks (Snowdon's summit comes out about 40 m low). At HF this matters little; check the
  profiles near the antenna against what you know of the site.
- The tiles are downloaded from Amazon Web Services when you press **Fetch terrain**, which
  tells it roughly which area you are looking at, and kept in the cache folder
  (`~/.cache/wireant/terrarium` on Linux) so the same area needs no download next time.
- Patterns take longer to calculate with fetched terrain (about a second for a dipole with 36
  profiles), as every profile has dozens of slopes.

How it works:

- Low rays reflect from the ground hundreds of metres to kilometres away, so the terrain there
  sets a site's low-angle pattern. Each straight piece of a profile reflects with its own slope.
  A ridge in front cuts the direct ray and the reflected rays as a knife edge. A hilltop or a
  slope falling away in front lowers the takeoff angle and raises the low-angle gain; a ridge in
  front lowers it.
- Toward each bearing the terrain is taken as the same across the ray (two-dimensional). Every
  piece has the ground constants set above.
- Terrain changes only the patterns. The impedance, currents and efficiency come from the ground
  under the antenna, so set that to the ground the antenna stands on (level, or sloping).
- Nothing is shown at or below the horizon.
- At the lowest angles, a few degrees from grazing, the results are approximate. A ridge can even
  raise the gain there a little, by breaking up the ground reflection's cancellation (as it does
  in reality).
- Terrain needs the **wiremom** engine and can't be combined with a second ground.

The View tab draws the profiles as brown lines from the origin.

## Wires touching or in the ground

- A wire end at Z = 0 connects to the ground. Over perfect ground this is exact.
- Over real ground, a wire that just stops at the surface is a poor model: a single point contact
  into soil can't be calculated well. The program warns about it. Add a ground rod or radials.
- With the **wiremom** engine, wires can run below Z = 0 over real ground. Ground rods, buried
  radials and counterpoises are modelled in full, including the loss in the soil.
- Wires lying on the ground at Z = 0 (radials laid on the soil) are modelled 1 cm into the soil.
  This is close to the real thing for bare or insulated wire lying on the soil. A wire's field
  reaches only a few wire diameters, so one touching the soil carries much the same wave as a
  buried one. Checks with an exact model of a wire resting on the surface found radiated shares
  within a few points of this model. A wire held even 1 cm above the soil behaves quite
  differently, so expect a big change between Z = 0 and Z = 0.01. Wires rising from Z = 0, such
  as a vertical, keep their feed point at the surface; a short stub in the soil joins them to the
  radials.
- Keep other wires at least three wire radii away from the surface, above or below. Closer than
  that, no thin-wire engine (NEC included) is reliable, and the program warns about it.
- The **External NEC** engine (NEC-4 or later) also takes wires below Z = 0 over real ground. A
  wire that goes through the surface gets a segment end at Z = 0, as NEC needs. Wires lying in
  the surface at Z = 0 can't go to NEC: put them a little into the soil and join them with a short
  wire.
- NEC-2 and MININEC can't model wires in the ground.

## Second ground

Tick **Second ground** when part of the surroundings is different, such as the sea next to a
portable station, a salt marsh, or a cliff edge.

- **Boundary**: a **straight line** or a **circle round the origin**.
  - For a straight line, set the **Distance (m)** from the origin and the bearing (**toward bearing**)
    in which the second ground lies. The bearing is an azimuth: 0° = +X, 90° = +Y.
  - For a circle, set its **Radius (m)**. The second ground lies outside it.
- **εr** and **σ**: the second ground's own values, with presets.
- **Drop (m)**: how far its surface lies below the main ground, for a cliff or bank.
- **Transition**:
  - **Fresnel zone** (wiremom and MININEC): each ray's reflection is shared between the two
    grounds, so a nearby shore blends in smoothly.
  - **reflection point (NEC-2)**: each ray uses the ground where it reflects.

The second ground affects only the pattern, as in NEC-2. The impedance and ground loss come from
the main ground, so make that the ground the station stands on. The View tab draws the boundary
in blue.
