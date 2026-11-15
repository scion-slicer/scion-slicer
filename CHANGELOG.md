# Changelog

All notable changes to Scion Slicer. Versions follow [semantic versioning](https://semver.org); the plugin, the npm packages and the server always share one version.

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
