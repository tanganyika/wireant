# Tools

The **Tools** menu holds the resonance finder, the match calculator, the stack / bay array
builder, the great circle map, the RF exposure check, the PDF report and, with an external NEC program set, the engine comparison report.

## Resonance finder

The resonance finder looks for every frequency in a range where the reactance is zero, then
measures each resonance accurately.

1. Set **From**, **to** and the number of **points**. The range starts around the antenna's frequency.
2. Set the SWR for the bandwidth (**bandwidth at SWR**, usually 2).
3. Press **Find resonances**.

For each resonance it shows:

- The type: **series**, where R is low, or **parallel**, where R is high.
- R and the SWR against the SWR reference.
- The SWR bandwidth against the reference impedance, and the bandwidth once matched to the
  resonance's own resistance.
- Q, worked out as f·|dZ/df| / 2R.

Plots of SWR and of R and jX are shown too. **Use frequency** makes that resonance the antenna's
frequency. **Match…** opens the match calculator with its impedance.

The bandwidth edges are found by extra calculations, so they are accurate even when the band is
narrower than the point spacing. Large models take a while: about a second per calculation at
1,350 unknowns. A bar shows the sweep, then the refining, with the time so far and roughly how
long is left; **Stop** ends the search.

## Match calculator

The match calculator designs a match from an antenna impedance to a target, 50 Ω by default.
The impedance can be typed in, taken from the last calculation, or sent from the resonance finder.

It offers:

- **L networks**: every arrangement of one coil and one capacitor, series or shunt first, with
  values. Each is marked as low-pass or high-pass.
- **Gamma match**: the rod length and series capacitor, from the element and rod diameters and
  their spacing. This uses the ARRL Antenna Book's transmission-line model, which is approximate,
  so check the result by modelling the rod, or add it to the antenna (below), which does that
  for you.
- **Coax sections**:
  - A Regier series section of a chosen impedance (75, 93, 35 Ω and others).
  - A length of feeder to a point where the impedance is purely resistive, then a quarter-wave
    transformer.

  Lengths are given in electrical degrees, and in metres for the **velocity factor** you set.

### Adding a match to the antenna

Choose a solution with its round button, then press **Add to the antenna**. Use **From last
calculation** first, so the match is for this antenna's own impedance. The source moves to the
transmitter side of the match, and **↶ Undo** on the View tab (Ctrl+Z) takes it all out again.

- **L network**: the coil and capacitor become parts on a new line (see Transmission lines on the
  Geometry tab), from the feed point to a new short feed wire that carries the source.
- **Gamma match**: the driven element is split at the feed and at the strap, and the rod, strap and
  a short feed wire with the source and series capacitor are added as wires. The rod goes below a
  horizontal element. The formula is only a starting point, so the rod length and the capacitor
  are then found with the antenna's own calculation.
- **Coax sections**: the lines are added from the feed point, joined by short wires, with the
  source on the last one.

L networks and lines need the wiremom or NEC-2 engine. The parts are ideal (no loss).

## Stack / bay array

**Tools → Stack / bay array…** turns the antenna into an array of copies of itself: **Stacked**
one above another, and/or **Bayed** side by side across the beam. **Beam towards** is the
direction of the beam (filled in from the antenna); bayed copies sit across it. The copies are
centred on the original, and their sources, loads, lines and plates are copied with them.

Every copy is fed in phase with the same current, as through an ideal power splitter and equal
phasing lines, so the result shows what the array itself can do; each copy's impedance in the
Calculate results then includes the coupling to its neighbours.

The spacings start at the usual compromise between gain and side lobes, from the antenna's −3 dB
beamwidth in free space (DL6WU): D = λ / (2·sin(BW/2)), using the vertical beamwidth for stacking
and the horizontal one for baying. Where the pattern is round in that plane (a dipole broadside,
a vertical) 0.75 λ is used. **Suggest spacings** works them out again.

**Make the array** puts it in place of the antenna; **Undo** brings the single antenna back. Two
antennas give close to 3 dB more gain and half the beamwidth in the direction they are spaced; four
in a square, close to 6 dB.

## Great circle map

**Tools → Great circle map…** lays the antenna's horizontal pattern from the last **Calculate**
over a world map centred on your station, so you can see where the beam lands. The map is
azimuthal equidistant: every straight line from the centre is a great circle path, the rings are
5000 km apart, and the rim is the far side of the world.

- **Locator**: your Maidenhead locator (4, 6 or 8 characters, e.g. IO91wm). It is remembered.
- **+x axis points to**: the compass bearing of the model's +x axis. Drag the **N** arrow on the
  top view to point at North instead, or click a place on the map to turn the antenna so its
  beam points there.
- **Pattern size** scales the pattern on the map.

The pattern is the azimuth cut at the elevation the Calculate tab uses (normally the elevation of
the maximum). The solid blue line is the beam's short path and the dashed line its long path.
Move the pointer over the map to read the locator, bearing and distance there and the antenna's
gain in that direction. Coastlines: Natural Earth 1:110m (public domain).

## RF exposure

**Tools → RF exposure…** maps the electric (E) and magnetic (H) field strength around the
antenna and compares it with the reference levels of an exposure standard, to help with the RF
exposure assessments licence conditions ask for. It works from the last **Calculate**, at its
frequency.

- **PEP into the antenna**, **Mode** and **On air** give the average power the limits apply to.
  The mode is the share of the PEP a transmission averages to (SSB voice about 20 %, CW 40 %,
  FT8 and FM 100 %); on air is the share of the averaging time spent transmitting (6 minutes
  for ICNIRP 1998, 30 minutes for ICNIRP 2020 and the FCC's general population). Leave out
  feed-line loss to stay on the safe side.
- **Standard**: ICNIRP 1998 general public (used in the EU and by Ofcom in the UK), ICNIRP 2020
  general public, or the FCC's general population limits. The limits at the frequency are shown;
  check them against your regulator's own figures.

A point's share of the limit is the larger of (E / E limit)² and (H / H limit)², which compares
power densities, so close to the antenna, where E and H are no longer in step, both count.

**Show → 3D around the antenna** (the default) works the field out all round the antenna and
draws the surfaces where it reaches the limit (red: inside it you are over the limit) and, if you
tick them, 30 % and 10 % of it, with the antenna and the ground. Drag to turn it, right-drag to
move it, use the wheel to zoom and double-click to reset, as on the 3D pattern; **Top**, **Front**
and **Side** turn it to those views. The note above says how far from the antenna the limit
reaches and, over ground, how low the zone comes compared with head height (2 m). The power, mode
and standard change the surfaces straight away; if a surface grows past the space worked out, a
note asks you to press **Calculate again**.

**Show → Slice through it** colours one plane by the share of the limit:

- **Plane**: a level plane at a height (2 m is about head height), or an upright plane through the
  middle of the antenna along x or y. **Reaching** sets how far it goes past the antenna.

Press **Calculate fields**. Red is over the limit. The note above the map gives the farthest
distance from the antenna at which the limit is reached, and the strongest point at least 0.5 m
from the wires; hover over the map to read any point.

The fields come from the calculated currents, so they suit every engine. In free space and over
perfect ground they agree with NEC-2's own near fields to within 0.1 dB. Over real ground the
reflection is approximated with plane-wave reflection coefficients, which is within about 1 dB of
NEC-2's full ground calculation a few metres up and rougher right at the ground. Plates aren't
included yet.

## Antenna report (PDF)

**Tools → Antenna report (PDF)…** writes an A4 report containing:

- The antenna's settings.
- A picture of it as on the View tab, with wire numbers and current colours.
- Its loads, lines, plates and comment.
- The latest calculation, its power budget and the results history.
- The horizontal and vertical patterns.
- The resonance finder's and match calculator's current results.
- RF exposure maps: a level plane and an upright plane around the antenna, with the power,
  standard, height and reach set in Tools → RF exposure (100 W average, ICNIRP 1998, 2 m and
  10 m if you haven't changed them).

The save dialog suggests the antenna file's name ending in `.pdf`. Once saved, the report opens in
your PDF viewer.

The Matching section matches the impedance you gave the match calculator. If you haven't used it,
the section matches the latest calculation's impedance instead, and it says so when the antenna is
already matched.

## Engine comparison report (PDF)

**Tools → Engine comparison report (PDF)…** is there once an external NEC program is set (File →
Preferences). It calculates the antenna with wiremom and with your NEC program, and writes an A4
report with the two side by side:

- Impedance, SWR, gain, F/B, the direction of the maximum, efficiency and time, with the
  difference between them.
- The horizontal and vertical patterns of both on the same plots, against the higher of the two
  maxima, so a difference in gain shows as well as one in shape.
- SWR, gain, R and jX over a 21-point sweep from 5% below to 5% above the antenna's frequency.
- Each engine's notes, and any error if one of them couldn't calculate the antenna.

It needs both engines to run the antenna, then the sweep, so it takes a while on a large model. A
bar shows how far it has got, and **Stop** ends it without writing the report. The save dialog
suggests the antenna file's name ending in `engine comparison.pdf`.
