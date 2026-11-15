# Design: one agent plugin for every coding agent

Status: proposed, not built. Researched November 2026.

## Goal

Ship Scion Slicer as a plugin that bundles skills with the MCP server, and have the same
plugin install in Claude Code, GitHub Copilot (CLI, VS Code and the Copilot app), OpenAI
Codex and Cursor. The server is distributed through npm. Users should find out when a new
version exists without us depending on each client's update settings.

## What each client reads

| | Copilot | Codex | Cursor | Claude Code |
|---|---|---|---|---|
| Plugin manifest | root `plugin.json` (Agent Plugins); it wins over `.claude-plugin/` | root `plugin.json` is the portable entry point; `.codex-plugin/` is for compatibility | root `plugin.json` (Agent Plugins) or `.cursor-plugin/plugin.json` | `.claude-plugin/plugin.json` only |
| MCP config | `mcp.json` | `mcp.json` (`type` required) | `mcp.json` | `.mcp.json` |
| Plugin root variable | `${PLUGIN_ROOT}` | `${PLUGIN_ROOT}`, plus `CLAUDE_*` aliases | only `${CURSOR_PLUGIN_ROOT}` | `${CLAUDE_PLUGIN_ROOT}` |
| Marketplace | reads `.claude-plugin/marketplace.json` | reads `.claude-plugin/marketplace.json` or `.agents/plugins/marketplace.json` | team import of a git repo, or `.cursor-plugin/marketplace.json` | `.claude-plugin/marketplace.json` |
| npm as a plugin source | no | yes | no | yes (install scripts never run) |
| Updates | `copilot plugin update`; `/plugin` flags newer versions | `codex plugin marketplace upgrade` | manual refresh; automatic only for GitHub imports with the GitHub App | only when `version` changes; background auto-update is off by default for third-party marketplaces |

All four read skills from `skills/<name>/SKILL.md`.

## Layout

```
.claude-plugin/
  marketplace.json          # one marketplace for Claude Code, Codex and Copilot
plugin/scion-slicer/
  plugin.json               # Agent Plugins 1.0: Copilot, Codex, Cursor
  mcp.json                  # Agent Plugins 1.0
  .claude-plugin/
    plugin.json             # Claude Code
  .mcp.json                 # Claude Code
  skills/
    check-a-print/SKILL.md
    ...
npm/
  mcp/                      # @scion-slicer/mcp: the launcher
  mcp-darwin-arm64/         # one package per platform, binary only
  ...
scripts/release.mjs         # stamps one version everywhere
```

The plugin lives in a subfolder on purpose. When a plugin sits in a subdirectory, Claude Code
fetches only the submodules inside it, so plugin users never clone DragonFruit.

The two manifests and the two MCP files say the same thing in two formats. A test checks
that they match (name, version, description, server command) so they cannot drift.

## MCP entry

Every MCP file runs the server through npx, pinned to the plugin's own version:

```json
{ "command": "npx", "args": ["-y", "@scion-slicer/mcp@0.2.0"] }
```

No path variable is needed, which avoids Cursor's `${CURSOR_PLUGIN_ROOT}`-only quirk. It also
keeps the skills and the server on the same version, because the plugin's version decides
the server's. People who use the server without the plugin run the same command in any MCP
client.

The cost is that the first run needs network access and Node, and it can take a few seconds.
After that npx serves the package from its cache.

## npm packaging

We use per-platform packages, the pattern esbuild and others use:

- **`@scion-slicer/mcp`** is a small JavaScript launcher. It works out which platform package
  is installed and runs its binary with stdio passed through.
- **`@scion-slicer/mcp-<os>-<cpu>`** each hold one binary and declare `os` and `cpu`, so npm
  installs only the matching one. The launcher lists them all as `optionalDependencies`, each
  pinned to the exact same version.

We rejected a post-install download, which is how cargo-dist's npm installer works. It
breaks wherever install scripts do not run: `--ignore-scripts`, Bun by default, locked-down
CI, and Claude Code's npm plugin source.

Releases are built by GitHub Actions on a `vX.Y.Z` tag: one job per target, then publish the
platform packages, then the launcher last. Publishing uses npm trusted publishing, so no
token is stored and each package gets a provenance statement.

## One version everywhere

`scripts/release.mjs X.Y.Z` writes the version into:

- `Cargo.toml` (workspace) and `Cargo.lock`
- every `npm/*/package.json`, including the launcher's `optionalDependencies`
- both plugin manifests
- the version pinned in both MCP files
- the marketplace entry

It then adds a CHANGELOG section and creates the tag. CI fails the release if any of these
disagree with the tag.

## How users learn about updates

The clients cannot be relied on: Claude Code users have to turn on auto-update for a
third-party marketplace, Cursor refreshes by hand, and Copilot only flags new versions when
someone opens `/plugin`. So the server tells them itself.

**The check**
- Once a day at most, in the background after startup, so it never delays a tool call.
- Asks `https://registry.npmjs.org/@scion-slicer/mcp/latest` for the version number only,
  with a 2 second timeout.
- Caches the answer and the time of the check in the plugin data folder
  (`${PLUGIN_DATA}` / `${CLAUDE_PLUGIN_DATA}`), or in the user cache folder when run without
  a plugin.
- Silent on any failure. Never retries in a loop.
- Off when `SCION_SLICER_NO_UPDATE_CHECK=1` is set, and when `CI` is set.
- Sends nothing about the user or their files. The request is a plain GET.

**Where the notice shows up**
- **The first tool result of a session** gets one extra line when a newer version exists,
  with the update command for the client in use. This is the channel that actually reaches
  people, because the agent reads it and can tell the user. It appears once per session, not
  on every call.
- **An MCP log message** (`notifications/message`, level `notice`). Clients may show it.
  This needs the server to declare the logging capability.
- **stderr**, which ends up in client logs.
- **An `about` tool** that returns the running version, the latest known version, and the
  changelog link.

**Outside the server**
- The README tells Claude Code users how to turn on auto-update for the marketplace.
- Every release has GitHub release notes taken from the CHANGELOG.

## Platforms

The engine has only been built and tested on Apple Silicon macOS. DragonFruit's toolchain
file lists only Apple targets. Linux x64/arm64 and Windows x64 need a CI build and the test
suite to pass before we publish packages for them. Until then the launcher prints a clear
"no build for this platform yet" message with a link to build from source.

## Open questions

1. Which platforms ship in the first release.
2. Whether the one-line update note in tool results is acceptable, or whether it should be
   opt-in.
3. The `@scion-slicer` npm org has to be created by the project owner before the first
   publish.
4. Codex's public plugin directory only accepts remote HTTPS MCP servers. Installing from
   our own marketplace with a local npx server should work, but needs testing on the Codex
   CLI before we document it.
5. Whether to add a Cursor marketplace file now or rely on team git imports at first.

## Sources

- Claude Code: [plugins reference](https://code.claude.com/docs/en/plugins-reference),
  [create a marketplace](https://code.claude.com/docs/en/plugin-marketplaces),
  [host and maintain](https://code.claude.com/docs/en/plugins/host-marketplace),
  [marketplace reference](https://code.claude.com/docs/en/plugins/marketplace-reference)
- Codex: [package your plugin](https://developers.openai.com/plugins/build/plugins),
  [submit a Claude Code plugin](https://developers.openai.com/plugins/guides/submit-claude-plugin)
- Copilot: [CLI plugin reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference),
  [Agent Plugins 1.0 announcement](https://github.blog/changelog/2026-08-12-agent-plugins-1-0-in-vs-code-copilot-cli-and-the-copilot-app),
  [VS Code agent plugin reference](https://code.visualstudio.com/docs/agents/reference/agent-plugin-reference)
- Cursor: [plugins](https://cursor.com/docs/plugins), [skills](https://cursor.com/docs/skills)
- Agent Plugins: [spec 1.0.0](https://raw.githubusercontent.com/agentplugins/agent-plugins-spec/main/spec/1.0.0.md),
  [agent-plugins.org](https://agent-plugins.org)
- npm: [cargo-dist npm installer](https://axodotdev.github.io/cargo-dist/book/installers/npm.html),
  [Sentry: publishing binaries on npm](https://blog.sentry.io/publishing-binaries-on-npm/)
- MCP: [logging](https://modelcontextprotocol.io/specification/2025-11-25/server/utilities/logging)
