---
name: slice-resin-print
description: Slice an STL into a file a resin printer can print (.goo for ELEGOO printers), using the Scion Slicer list_printers, list_resins and slice tools. Use when someone wants to slice a model, make a print file, prepare a part for their resin (MSLA) printer, pick a resin profile or exposure settings, or compensate for elephant's foot on the bottom layers.
---

# Slice a resin print

The `slice` tool turns a binary STL into a print file: every layer drawn at the printer's own resolution with DragonFruit's engine, written in the printer's format. For ELEGOO printers it writes `.goo` files the way ELEGOO SatelLite writes them, so they open in SatelLite and print with SatelLite's timing.

## 1. Check the part first

Run `check_print` on the STL (see the check-resin-print skill) unless the user has just done so. A resin trap or a suction cup is cheaper to fix before slicing than after a failed print. If it finds warnings, tell the user and ask whether to slice anyway.

## 2. Pick the printer and resin

- Call `list_printers`. When ELEGOO SatelLite is installed, the printers and resin profiles are SatelLite's own, so they match what the user sees there. Otherwise built-in profiles are used; say so.
- Call `list_resins` for that printer. The profile marked `active` is the one SatelLite last used. If the user names a resin, find it in the list; if it isn't there, show the closest names and ask. Don't guess exposures.
- Tell the user which profile you'll use, with its layer height and exposures, before slicing.

## 3. Slice

Call `slice` with `stl_path` and, if needed:

- `resin` and `printer`: a name or a part of one that matches only one entry.
- `output_path`: by default the file goes next to the STL with a `.goo` extension. The tool refuses to replace an existing file; only pass `overwrite: true` when the user says to replace it.
- `rotate_deg`: turns the part about the vertical axis. Orientation (which side is down) comes from the STL itself; this tool doesn't tip parts over or add supports.
- `position_mm`: where the corner of the part goes, from the corner of the screen. Leave it out to centre the part.
- `outline_compensation_mm` and `hole_compensation_mm`: elephant's-foot compensation on the bottom layers. Negative values shrink outlines and open holes. They fade out over the transition layers unless `fade_compensation` is false. Only use them when the user asks or has measured their printer; typical values are a few tenths of a millimetre.

## 4. Report

Give the user the file's path, the layer count, the print time, and the resin volume and weight from the result. Pass on every line in `notes`: they say when built-in profiles were used and that Scion Slicer's print files are new.

The part must already have any supports it needs: this tool doesn't add them. If the part is too big for the screen, the tool says so; suggest turning it or scaling it in a modelling tool.
