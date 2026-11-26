# Design: slicing by height ranges into CTB

Status: built (crates `scion-slicer-ctb` and `scion-slicer-core::ranges`). Checked against
Chitubox's files; not yet printed.

## Why

Per-layer settings in a `.goo` file look unsupported on Elegoo's firmware: two DragonFruit
prints with the per-layer flag stopped early on a Saturn 4 Ultra 16K, UVtools has traced
the same symptom to that flag, and Chitubox Pro won't export its "Slicing by Height"
(multi-parameter) prints to `.goo` at all, only to `.ctb`. So per-layer settings and
variable layer heights go into encrypted CTB v5 files, the way Chitubox Pro writes them.

The first use is a mold top whose thin ridges peeled off at their base: 50 µm layers up to
4.00 mm, then 100 µm with a 5 s exposure, a 2 s rest before lift and a 5 s rest after
retract.

## Sources

Three files of the same mold top, all from Chitubox (not in the repository):
- Chitubox Pro, two height ranges (the target);
- Chitubox, one range, `.ctb` (baseline);
- Chitubox, one range, `.goo`.

Record layouts follow UVtools' `CTBEncryptedFile`; the encryption follows DragonFruit's
MIT-licensed CTB plugin. `examples/ctb_dump` prints every field of a file;
`examples/ctb_compare` compares two files field by field and pixel by pixel.

## The format, as Chitubox writes it

In file order:

1. 48-byte header: magic `0x12FD0107`, settings size and offset, version 5, signature size
   and offset, then constants (1, 1, 42).
2. 288-byte settings block, AES-256-CBC with the fixed key and IV.
3. Two previews, each a **16-byte** record (width, height, offset, length) followed by the
   image. Chitubox writes the same 500 × 210 picture twice. (DragonFruit writes 32-byte
   records.)
4. The machine name, the 320-byte disclaimer text, and the resin block: 4 zero bytes, the
   RGBA colour, offset and length of the resin type, resin name and profile name strings,
   the density as a float, 4 zero bytes, then the three strings.
5. The layer pointer table: 16 bytes a layer (offset, page, 88, 0).
6. Each layer's 88-byte record followed by its image.
7. 8 bytes: a float and a zero. The float is 0 in the multi-range file and 103.9 in the
   baseline; its meaning is unknown. We write zeros.
8. The 32-byte signature: SHA-256 of the settings' checksum value (0), AES-encrypted.
9. **A CRC-32 of everything before it**, little-endian. DragonFruit writes a constant here.

Layer images are run-length encoded (7-bit value, optional 1–4 byte run length) and then
scrambled with a XOR stream seeded by the settings' `LayerXorKey` and the layer index.
Chitubox picks a random key and also AES-encrypts a 512-byte slice of about one layer in
ten (`EncryptedDataOffset/Length`). The reader handles both; the writer scrambles but
doesn't encrypt slices, which the format allows.

Settings fields that matter:

- `PerLayerSettings` is 64 (0x40) when layers carry their own settings; 0 otherwise.
- `LastLayerIndex` holds the **layer count**, not count − 1 (UVtools displays count − 1).
- `Unknown6` is the sum of every layer's exposure and three rests, as a 32-bit float
  (931.0 s for the mold top's two ranges, 989.0 s for one range).
- `Unknown7` varies by file: 37335 for both mold-top files, 5071 for the test pins. It
  isn't a CRC-16, CRC-32 or sum of the previews or the STL; its meaning is unknown, and we
  write 37335. `Unknown8` = 3, `Four1` = `Four2` = 4, `Padding7` = 1.
- The header's lift and retract fields hold only the first stage (0.05 mm); each layer's
  record holds the real two-stage moves.
- `MaterialMilliliters` uses the header's layer height for every layer, so a 100 µm range
  counts at half its volume. We count each layer at its own height.
- `PrintTime` doesn't follow from the layers' times in any way found yet (1368 s for the
  mold top; ours comes to 1303 s). We write our own estimate.

## Height ranges

Rules found in Chitubox Pro's file, all reproduced exactly:

- A range starts where the one below ends; its first layer's top is one of its layer
  heights above that: 4.00, then 4.10 at 0.1 mm.
- **z = n × h in 32-bit floats**, where n counts layer heights h from the plate. The
  second 0.1 mm layer is 42 × 0.1 = 4.2000003. This matches all 213 z values in both
  Chitubox files.
- The last range takes as many layers as reach the part's top, rounded up. The mold's
  ridges end at 5.75 mm, so the last layer runs 5.70–5.80; sliced at its middle (5.75, the
  top face), it's blank. Chitubox keeps that blank layer, and so do we by default
  (`drop_blank_last_layer` leaves it out).
- Bottom and transition layers are only in the first range. Layers 0–4 use the bottom
  motion (lift 0.05 mm at 45 + 4.0 mm at 240 mm/min); everything after uses the normal
  motion (0.05 at 60 + 4.5 at 240; retract 240, then 2 mm at 60).
- Images are mirrored left-right. Chitubox's own `.goo` export says mirror X too, where
  SatelLite mirrors top-bottom; the two differ by a half turn, so parts print the same way
  round either way. DragonFruit's CTB preset mirrors Y.

## Chitubox's pixel grid

Chitubox centres a part on the screen and lights a pixel when its **corner** is inside
the part, where DragonFruit's engine tests the pixel's **centre**. So on a 4 mm pin
(285.7 × 210.5 pixels) Chitubox lights from ⌈left⌉ to ⌊right⌋ and gets 286 × 211 every
time, while centre sampling gives 285 or 286 × 210 depending on where the pin falls. It
isn't a fatter edge: the two grids are offset by half a pixel. Placing the part one and a
half pixels from our plain centred position (`Placement::ChituboxCentered`, which a centred
CTB slice now uses) reproduces Chitubox's pixels. The only remaining difference is an
edge that falls exactly on a pixel boundary: Chitubox counts the boundary pixel as inside,
the engine doesn't.

## How close we are

Compared pixel for pixel at the same positions, with no alignment step:

| | Ours vs Chitubox Pro, mold top, two ranges | Ours vs Chitubox Pro, test pins, two ranges |
|---|---|---|
| Layers, z, every layer-record field | identical (98) | identical (120) |
| Signature and CRC-32 | valid | valid |
| Settings | the same except offsets (our preview is smaller), the XOR key and time stamp, print time, resin volume and `Unknown7` | the same |
| Plate layers, pixel overlap | 99.998% mean (6–8 pixels of 58 million differ) | — |
| Ridge layers / pins, pixel overlap | 99.983% mean, 99.894% lowest (the V tip) | 99.913% (one pin edge exactly on a pixel boundary) |

Our single-range slice matches Chitubox's single-range file the same way. Rewriting
Chitubox Pro's own layers with our writer (`examples/ctb_rewrite`) gives identical layer
records and settings apart from offsets and print time.

## DragonFruit's CTB encoder

Sliced with DragonFruit's Saturn 4 Ultra 16K preset (encrypted v5, "tilting" mode, mirror
Y) and the same profile, against Chitubox's one-range file (`examples/df_ctb`):

- The trailing CRC-32 is wrong: a constant instead of the file's checksum.
- Several settings land in the wrong fields or hold made-up values: `ChecksumValue`
  0xCAFEBABE, `MachineZ` set to the part's height, time stamps in `Unknown1/2`, lift values
  in the material fields, `Four1/2` not 4, `Unknown6/7/8` and `LastLayerIndex` off.
- The per-layer flag is set even though every layer is the same.
- Each layer's motion is one 0.05 mm lift and retract at 60/150 mm/min where Chitubox
  writes the two-stage 4.05–4.55 mm moves; "tilting" mode replaces them.
- The print time counts only exposures (362 s).
- The images mirror Y, and the plate layers match Chitubox's to 99.8%.

Whether the printer reads any of these fields is untested, but a CTB writer for this
printer is better off matching Chitubox, and these are the places DragonFruit doesn't.
