# Changelog

All notable changes to Scion Slicer. Versions follow [semantic versioning](https://semver.org); the plugin, the npm packages and the server always share one version.

## Unreleased

- **Slicing.** New `slice` MCP tool: turns a binary STL into a `.goo` print file for ELEGOO printers, with optional rotation, position and bottom-layer (elephant's foot) compensation, and reports the layer count, print time, volume and weight.
- New `list_printers` and `list_resins` tools. When ELEGOO SatelLite is installed, they use its printers and resin profiles, including the lift settings SatelLite hides from its Parameters tab; otherwise a built-in Saturn 4 Ultra 16K with two profiles.
- New `slice-resin-print` skill.
- **Slicing by height.** New `scion-slicer-ctb` crate reads and writes encrypted CTB v5 files with per-layer settings, laid out as Chitubox Pro writes them (including the trailing CRC-32 DragonFruit gets wrong). `ranges::slice_ranges_to_ctb` slices a part in height ranges, each with its own layer height, exposure, rests and motion, following Chitubox Pro's z and layer-count rules exactly.
- New `scion-slicer-goo` crate: writes `.goo` files exactly as ELEGOO SatelLite 1.0.2.43 does (version V3.0, advance mode off, SatelLite's layer encoding, transition ramp and print-time estimate), checked byte for byte against ten reference slices made with SatelLite.

## 0.1.2

- The plugin now runs the latest published server (`npx -y @scion-slicer/mcp@latest`) and passes its own version to it. Before, it pinned the server to its own version, and anyone installing or updating the plugin between a release reaching GitHub and reaching npm got a server that would not start ("No matching version").
- Server fixes now arrive at your next session without a plugin update. The update notice and `about` now compare the plugin's version with the latest release, since the skills still update with the plugin.
- `about` reports the plugin's version.

## 0.1.1

No changes to the server or the plugin's behaviour. This release moves publishing onto npm trusted publishing.

- Published from the release workflow through npm trusted publishing, with a provenance statement for every package. 0.1.0 was published by hand and has none.
- Releases run only from a `v*` tag and wait for a maintainer's approval before publishing.
- Added a security policy (`SECURITY.md`) with private vulnerability reporting.
- README: install commands now work, since the packages are on npm.

## 0.1.0

First release.

- `check_print` MCP tool: resin traps, suction cups, peel alerts and ledges, each with a cause and a suggested action.
- `about` MCP tool: the running version, the latest version on npm and how to update.
- Agent plugin for Claude Code, GitHub Copilot, Codex and Cursor, with the `check-resin-print` skill.
- npm packages: `@scion-slicer/mcp` and one binary package per platform.
