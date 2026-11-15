---
name: check-resin-print
description: Check a model for resin 3D printing problems before slicing, using the Scion Slicer check_print tool, and explain what to fix. Use when someone wants to know whether an STL will print on a resin (MSLA/SLA/DLP) printer, asks about resin traps, suction cups, cupping, blowouts, peel forces or overhangs, or is about to slice or hollow a part.
---

# Check a resin print

The `check_print` tool slices a mesh into layers and looks for the problems that fail resin prints. It works from the layers, the way the printer sees the part, so it catches problems that are hard to see in a 3D view.

## Before you run it

1. **Get an absolute path to a binary STL.** ASCII STL is not read yet. If the user has another format (OBJ, 3MF, STEP), say so and ask them to export a binary STL.
2. **Make sure it is in print orientation.** The tool treats the lowest point of the mesh as the part of the print against the build plate, and the layers go up from there. A part modelled lying flat but printed standing up, or upside down, gives the wrong answer. If you don't know how it will be printed, ask.
3. **Supports are not added.** The check sees the model as given. If the user's STL already includes supports, say that the findings include them.

## Run it

Call `check_print` with `stl_path`. The defaults (0.05 mm pixels, 0.05 mm layers) suit most printers. If the user tells you their printer's pixel size or their layer height, pass `px_mm` and `layer_height_mm`. Finer pixels are slower; large parts at small pixel sizes can take a while.

## Read the report

The report has the layer count, the part size and a list of `findings`. Each finding has a `kind`, a `severity` (`warning` or `info`), the layer range (layer 0 is against the plate), a `location_mm`, a `size` with its unit, a `cause` and an `action`.

Talk about them in this order, most damaging first:

| kind | What it means | Why it matters |
|---|---|---|
| `resin_trap` | A pocket of liquid resin sealed inside the part. | Uncured resin stays inside, can crack the part later, and adds weight and cost. |
| `suction_cup` | A hollow open toward the vat film (downward while printing). | Each layer lift has to pull against a vacuum. This causes blowouts, layer splits and failed prints. |
| `ledge` | An overhang, with how far it sticks out in mm. | Deep unsupported ledges sag or fail. Short ones usually print. |
| `peel_jump` | A layer whose area jumps sharply over the one before. | A sudden jump in peel force can tear the part off the supports or the plate. |
| `largest_layer` | The layer with the most cured area. | The highest peel force of the print. Useful when choosing lift speed and wait times. |

For each finding, say where it is in words the user can find on the model (near the top, the left wall, about 12 mm up), then the cause and the suggested action from the report. Use the numbers from the report and do not make up new ones.

If there are no findings, say so plainly, and mention what was not checked: supports, islands and wall thickness are not part of this check yet.

## Suggesting fixes

- **Resin trap:** add a drain hole at the lowest point of the pocket in print orientation, and a vent hole high up so air can get in.
- **Suction cup:** re-orient the part so the opening faces away from the film, or add a vent hole through the cup's closed end so air can get in.
- **Deep ledge:** add supports under it, re-orient, or change the model (a chamfer or fillet under the ledge).
- **Peel jump or large layer:** angle the part so each layer's area grows gradually, or use a slower lift and a longer wait on those layers.

These are suggestions. Don't change the user's files or printer settings unless they ask you to.
