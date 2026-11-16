# SatelLite reference slices: Saturn 4 Ultra 16K

Sliced with ELEGOO SatelLite 1.0.2.43 on 2026-11-15 to learn how SatelLite builds a `.goo`
file. Anti-aliasing is off in all of them. Every file can be explained by the rules below.

## The files

The models are plain boxes made for this: `small.stl` (20 × 20 × 2 mm, 40 layers),
`tall.stl` (20 × 20 × 10 mm, 200 layers) and `wide.stl` (150 × 90 × 2 mm, 40 layers).

Files 00–08 use SatelLite's default resin, with the visible settings typed in to match the
SirayaTech Fast (Navy Grey) profile:

| Setting | Bottom | Normal |
|---|---|---|
| Exposure | 30 s | 1.9 s |
| Wait Before Cure | 1 s | 0 s |
| Wait Before Release | 1.5 s | 0 s |
| Wait After Release | 0 s | 1 s |

Bottom layers: 5. Transition layers: 10. Layer height: 0.05 mm.

| File | Model | Change |
|---|---|---|
| 00-small-baseline | small | none |
| 01-small-baseline-again | small | none; saved a second time |
| 02-tall | tall | none |
| 03-wide | wide | none |
| 04-small-transition-5 | small | 5 transition layers |
| 05-small-transition-0 | small | 0 transition layers |
| 06-small-bottom-3 | small | 3 bottom layers |
| 07-small-bottom-exposure-20 | small | bottom exposure 20 s |
| 08-small-waits | small | bottom waits 2 / 3 / 4 s, normal 0.5 / 0.6 / 0.7 s |
| 10-wide-siraya-profile | wide | the real SirayaTech Fast (Navy Grey) profile |
| 11-small-siraya-profile | small | the real SirayaTech Fast (Navy Grey) profile |

A slice at 0.1 mm layer height wasn't made, because it needs the saved profile edited.

## Layers

Each layer is encoded as one chunk per run of equal pixels, using the shortest length
field that fits; the checksum is the one's complement of the byte sum. Re-encoding the
decoded pixels this way reproduces every layer of every file byte for byte: 520 layers
here, plus 2,430 in two earlier SatelLite files. The pixels are 0x00 or 0xFF only.

## Header and layer records

- Version `V3.0`, `PerLayerSettings` 0, `GrayScaleLevel` 0, anti-aliasing / grey / blur 0.
- Lift and retract heights and speeds, in the header and in every layer, are written as
  0.05. The second stage is 0 in the header and 0.05 in the layers.
- Each layer's pause position is 200.0 and its light-off delay 0.
- The three waits map like this:

  | SatelLite setting | Header field (bottom / normal) | Layer record slot |
  |---|---|---|
  | Wait Before Cure | `*WaitBeforeCure` | 4th float after exposure |
  | Wait Before Release | `*WaitAfterCure` | 2nd |
  | Wait After Release | `*WaitAfterLift` | 3rd |

- Transition layers use the **bottom** waits.
- Transition exposure: with `step = (bottom − normal) / transition_count`, transition
  layer k (1-based) is exposed for `bottom − k × step`, so the last transition layer is
  already at the normal exposure.
- Volume (mm³) = lit pixels × pixel width × pixel height × layer height.
  Grams = volume × density / 1000. Cost = grams × price per gram.
- The footer is always `00 00 00 07 00 00 00 44 4C 50 00`.
- SatelLite leaves leftover memory after the end of its strings (machine name, profile
  name, currency) and stamps the creation time, so two saves of the same slice differ
  only in those bytes.

## Print time

```
print_time = floor( Σ over layers of
                    exposure + the three waits + 2.5 + lift_mm / lift_speed + retract_mm / retract_speed )
```

The sum is taken in 32-bit floats. Speeds are in mm/min and converted to seconds.

- **2.5 s is a fixed tilt time per layer.** It doesn't depend on layer area: the small and
  wide boxes get the same time.
- **Lift and retract come from the resin profile**, not from the Parameters tab. The
  SirayaTech Fast (Navy Grey) profile has 5 mm at 65 mm/min up and 5 mm at 150 mm/min back,
  which adds 6.615 s per layer. SatelLite's default resin has none.
- This matches the header of all 12 SatelLite files tested, to the second, including
  `bottom-compensation-test_1_202611142128.goo` (1010 s).
