# Design: a `.goo` writer that matches ELEGOO SatelLite

Status: built (crate `scion-slicer-goo`, used by `slice` in `scion-slicer-core` and the MCP
`slice` tool). Matches SatelLite byte for byte in tests; not yet test-printed.

## Why

DragonFruit's `.goo` writer produces files the Saturn 4 Ultra 16K handles badly. Two
calibration prints sliced with it stopped early while the printer reported "Printing
Completed": one after layer 38 of 60, the other around layer 51 of 60. Its files differ
from what ELEGOO SatelLite writes in several ways:

- it sets the advance-mode flag, so the printer reads every layer's own settings;
- it writes version V1.2, which SatelLite 1.0.2.43 won't open;
- its print time leaves out the transition ramp and the tilt;
- its volume, weight and cost are 0;
- its layers are anti-aliased while the header says anti-aliasing is off, which makes
  them about 5× larger.

Rather than patch that writer, Scion Slicer gets its own, built to write what SatelLite
writes. Patching would mean carrying changes to DragonFruit's Elegoo plugin and asking its
maintainers to take them.

## Sources

- **SatelLite's own output.** The reference slices and the rules they show are in
  [`corpus/satellite/saturn-4-ultra-16k`](../../corpus/satellite/saturn-4-ultra-16k/README.md).
  Every rule there was checked against every SatelLite file we have.
- **Elegoo's published spec**, "Goo Format Spec V1.2" (github.com/elegooofficial/GOO,
  2023). SatelLite writes `V3.0` with the same layout. The spec names the fields; the
  reference slices show what SatelLite puts in them.

## What the spec settles

- **Header field 53 is "advance mode".** 0 means the printer uses the header's settings;
  1 means it uses each layer's own record. SatelLite writes 0; DragonFruit writes 1.
- The three header waits for bottom layers are "before lift", "after lift" and "after
  retract". SatelLite's UI calls them "Wait Before Release", "Wait After Release" and
  "Wait Before Cure".
- "Delay mode" 1 means static waits, 0 means the light-off time. SatelLite writes 1.

## Settings that never reach the file

A SatelLite resin profile holds more than the Parameters tab shows. These profile values
were compared against the files:

| Profile value | In the file? |
|---|---|
| Lift and retract distance and speed (5 mm at 65 / 150 mm/min) | No: the file has 0.05 placeholders. They only feed the print-time estimate. |
| Second-stage ("slow") lift and retract | No; 0 in this profile |
| First-layer exposure (50 s) | No: layer 1 gets the bottom exposure |
| Light-off delay (1 s) | No: written as 0 |
| Image blur, inner compensation, beam compensation, reduce point | They change pixels, not header fields |

So the writer needs the profile's motion settings only for the print time. Everything else
in the header comes from the visible settings, the printer and the resin price and
density.

## Design

The crate knows nothing about meshes. It takes finished layer images, 8 bits per pixel,
one layer at a time, and writes the file:

- `Machine`: the printer's fixed facts (resolution, display size, height, mirroring,
  names) and SatelLite's per-printer constants (the 2.5 s tilt, the pause position).
  `Machine::saturn_4_ultra_16k()` is the first entry; other Elegoo printers can be added
  the same way once their reference slices exist.
- `Resin`: the profile, meaning exposures, waits, layer counts, PWM, density, price and
  the motion used for timing.
- `GooWriter`: streams layers to any `Write + Seek` and fills in the header at the end,
  once the layer count, print time and volume are known. A 2,000-layer print never has to
  be held in memory.

Matching SatelLite exactly means:

- **Layers:** one chunk per run, shortest length field, 0x55 lead byte, and a
  one's-complement checksum.
- **Strings:** zero-padded. SatelLite leaves leftover memory there; we write zeros.
- **Header and layer records:** the rules in the corpus README.
- **Print time:** the rules in the corpus README, including the 32-bit float sum and
  rounding down.
- **Volume:** SatelLite takes it from the mesh: the small box is exactly 800 mm³, while
  counting pixels gives 800.0x. So the caller can pass the mesh volume, and the writer
  falls back to counting pixels.

Not matched, and why:

- **Software name and version:** we write "Scion Slicer" and our own version.
- **Preview images:** SatelLite renders the model in 3D. We write a top view of the
  printed area.
- **Anti-aliased layers:** SatelLite's gray encoding isn't worked out yet. The writer
  encodes gray pixels per the spec, but those files won't be byte-identical to
  SatelLite's.

## Tests

The reference slices are the tests. For each one, the test reads SatelLite's file, decodes
its layers, writes them again with the same settings, and compares. Every layer must be
byte-identical, and every header field must match except the software name and version,
the time stamp, the previews and the leftover string bytes.

## Checked on real parts

Beyond the reference boxes, two of the user's own prints were sliced with `examples/slice`
and compared with SatelLite's files of them (`examples/compare`). Neither part is in the
repository.

- **The bottom-compensation test plate** (60 layers), turned 48.81° as it sat in SatelLite:
  the print time (1010 s), volume and weight match exactly. The layer images match as they
  are to within 0.09% of the lit pixels (the angle was estimated, not known), and every
  mirrored or turned variant differs by millions: the images face the same way.
- **A production mold** (2,370 layers, 118.5 mm tall), rebuilt from SatelLite's own project
  file, so the mesh and its 22° turn are exactly what SatelLite sliced. Our file came to
  69,322,758 bytes against SatelLite's 69,321,395. Sampled layers differ in 245–1,055
  pixels out of about 16 million lit ones, at the edges. Slicing it took 5.1 s and 1.7 GB of
  memory on an Apple-silicon Mac.

  SatelLite's project file places the part 14.1 mm further along Y than where SatelLite
  actually printed it. Our placement matches SatelLite's printed position for parts loaded
  from STL (the reference boxes); the offset only matters when reading SatelLite projects.
