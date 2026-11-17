# Geometry

The Geometry tab holds the whole antenna. Changes take effect straight away; press **Calculate**
again to see their results.

At the top you give the antenna a **Name** and set its **Frequency (MHz)**. The wavelength (λ) is
shown next to it.

## Wires

Each row is one straight wire:

- **X1, Y1, Z1** and **X2, Y2, Z2**: the two ends, in metres.
- **R (mm)**: the wire's radius. This is half its diameter.
- **Seg**: how many segments the wire is split into. With **auto** ticked, the number comes from
  the automatic segmentation settings (see below).
- **Length (m)**: worked out from the ends.
- **Insulation (mm)**, **εr**: the thickness of the insulation and its relative permittivity. Insulation
  slows the wave on the wire, so an insulated wire resonates lower than a bare one. **VF** shows the
  velocity factor this gives in air. With the wiremom engine, insulation on wires in the ground or
  lying on it (buried or surface radials) is modelled for the soil around it. There the sleeve
  keeps the wire out of contact with the soil and slows the wave much more, so an insulated buried
  wire resonates far shorter than in air. The other engines treat insulation as it acts in air.
- 🗑 deletes the wire. Any sources and loads on it go with it.

Wires connect where their ends meet exactly, within a micrometre. A wire end at Z = 0 over
perfect or real ground connects to the ground.

**Insulation for all wires** sets the same insulation on every wire in one go, with a choice of
PVC, polyethylene or PTFE. **Remove all** takes it off again.

## Sources

A source is the feed point. Choose the **Wire** and the **Position** on it: begin, centre, end, or
a custom fraction of the way along. **Phase**, **Value** and **Type** matter only when there are
several sources, as in a phased array. The impedance and SWR shown are for the first source.

**Type** is **voltage** or **current**. A voltage source keeps its voltage (the value in volts),
and a current source its current (the value in amps), whatever impedance it meets. Phased arrays
are usually designed for given currents in their elements, so feed them with current sources.
With current sources, each calculation solves the antenna once per source first, to find the
voltages that give those currents, so it takes a little longer.

## Loads

Loads are components placed on a wire:

- **R+jX**: a fixed resistance and reactance.
- **L/C**: an inductor, a capacitor or both. With both, they are in series, or in parallel (a trap)
  when **parallel** is ticked. **Q** adds the coil's loss; 0 means lossless.
- **Coax choke**: coax wound into a single-layer air coil (an "ugly balun"). Set the turns, the
  former's diameter, the coax's diameter (▾ lists common cables) and the winding length (0 means
  the turns touch). The program works out the coil's inductance (Nagaoka) and self-capacitance
  (Medhurst) and models it as a parallel trap, so it blocks best around the resonance it shows.
  **Q** is the coil's loss: measured air-wound chokes suggest 10–30.

  Put the choke on the wire that stands for the outside of the coax shield. A feed line is
  modelled as a wire from the feed point running where the coax runs: the inside of the coax
  carries the feed, and that wire carries the common-mode current on the outside, which the
  choke stops. `.maa` and EZNEC files hold a choke as its trap; `.maa` and `.nec` files saved
  here also keep the choke's own values.

## Transmission lines

These are feed lines, phasing lines and stubs between two places on wires. They work with the
wiremom and NEC-2 engines only. For each line you set:

- **Z0**: the characteristic impedance.
- **Length**: 0 means the straight distance between the two ends.
- **Vel. factor**: about 0.66 for solid-PE coax and 0.8 for foam.
- **Loss**: in dB per 100 m at the calculation frequency. NEC-2 ignores loss.
- **Reversed**: the conductors are swapped at the far end, a half twist.
- **Parts**: coils and capacitors that the match calculator added. A part can sit across either
  end, or join the two ends in place of the line, and then the line's own values are unused.
  Delete the line to remove them. In `.nec` files they are TL shunt admittances and NT cards.

## Transformers

A transformer, a balun or unun such as the 49:1 of an end-fed half wave or the 9:1 or 4:1 of a
random wire, joins two places on wires like a transmission line: the **primary** is where the
feed comes in, the **secondary** is the antenna side. It works with the wiremom and NEC-2
engines only.

**➕ Feed through…** puts one in at the source: the source moves onto a small feed wire beside the
antenna, which feeds the primary, and the secondary goes where the source was. The small wire is
too short to radiate; it is the usual NEC way to feed a network. For an end-fed antenna, the
secondary sits on a short wire (a few tens of cm) between the end of the radiator and the
counterpoise or coax shield, so the model has the transformer's two terminals.

- **Ratio N:1**: the impedance ratio. The secondary sees N times the primary's impedance (the turns
  ratio is √N). Below 1 it steps up the other way.
- **Primary L**: the primary winding's inductance on its core, AL × turns². 🧲 works it out from a
  common toroid and the primary turns. 0 makes an ideal transformer. Too little inductance shunts
  the feed at the low end of the band.
- **Coupling k**: 1 means no leakage; twisted or interleaved windings on a toroid reach about
  0.98–0.995. Leakage spoils the match at the high end of the band.
- **Core Q**: the core's loss, as the windings' Q. #43 and #31 mix are lossy at HF (Q about 1–3).
  The power lost shows with the lines' in the power budget.
- **C across primary**: a compensating capacitor, often about 100 pF on a 49:1.
- **Reversed**: the secondary's ends swapped.

The transformer is modelled as two coupled windings, which covers baluns and ununs alike; whether
the two sides share a terminal is up to the wires around it. In `.nec` files it is an NT card
(its admittances at the file's frequency, which any NEC program reads) and a `CM` comment that
keeps its values.

## Plates

Plates are flat conducting sheets, such as a reflector, a ground screen or a car-body panel. They
work with the wiremom engine only. Each is a parallelogram, given by one corner and the two edges
that meet there. **Divisions** sets how finely it is meshed; 0 means automatic, about λ/10. A wire
that ends on a plate connects to it, so a monopole can be fed against a plate.

## Ground and wire

- **Ground**: **Free space**, **Perfect ground** or **Real ground**. See [Ground](ground.md).
- **Wire material**: copper, aluminium or iron, as wire or pipe, or **No loss**. **User wire** and
  **User pipe** take your own resistivity and permeability.
- **Wire ends** (wiremom only): **flat (solid rod)** gives each free wire end a flat cap that
  carries charge, as the end of a solid rod does, so a thick element acts about a radius longer at
  each end. **open (tube, as NEC)** leaves the ends open, as on a hollow tube and as NEC and MININEC
  model every end. On thick elements at VHF and UHF the choice can move a Yagi's band edge by about
  half a percent; use **open** to compare with NEC results. Choosing a **Wire material** sets it:
  a pipe is a tube, so its ends are open, and a wire is a solid rod with flat ends. Change it
  afterwards for tubes with metal end caps. EZNEC files, and NEC files from other programs, don't
  say which, so they come in with flat ends; `.maa` files follow their material, and wireant's own
  NEC files keep your choice.
- **Rear range for F/B**: how wide the rear region is, in azimuth and in elevation, when looking
  for the strongest rear lobe.
- **SWR reference (Ω)**: the feed line impedance used for SWR.

## Automatic segmentation

Automatic segmentation puts short segments near wire ends and junctions, where the current changes
fastest, and longer ones in the middle of a wire:

- **Shortest segment λ /** and **Longest segment λ /**: the segment length limits, as fractions of a wavelength.
- **Growth factor**: how quickly the segments lengthen away from an end.
- **Shortest segments at each end**: how many segments of the shortest length come first.

The NEC-2 engine always uses evenly spaced segments, because NEC-2 is inaccurate next to very
short segments.

MININEC and NEC-2 never get segments shorter than the wire's diameter: a wire whose own count
would need shorter ones gets fewer. wiremom models the wire as a tube, which stays accurate down to
segments of half a radius, so it keeps a file's own counts (EZNEC's 100 per element, say) on thick
elements.

## Comment

Free text saved with the antenna. It also appears in the PDF report.
