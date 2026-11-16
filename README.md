# Scion Slicer

An open, agent-driven resin slicer, built on the [DragonFruit](https://github.com/Open-Resin-Alliance/DragonFruit) slicing engine by the [Open Resin Alliance](https://github.com/Open-Resin-Alliance).

Scion Slicer exposes resin printing tools over the [Model Context Protocol](https://modelcontextprotocol.io), so an AI agent can check and prepare prints. It uses DragonFruit's engine, printer plugins and material profiles as they are, and adds its own tools on top.

## Tools

| Tool | What it does |
|---|---|
| `check_print` | Checks a binary STL in print orientation for resin traps, suction cups, peel problems and ledges. Each finding has its layer range, location, size, cause and a suggested action. |
| `list_printers` | The printers it can slice for: ELEGOO SatelLite's when it's installed, otherwise the built-in ones. |
| `list_resins` | A printer's resin profiles, with their layer height, exposures, layer counts and waits. |
| `slice` | Slices a binary STL into a print file (`.goo` for ELEGOO printers), with optional rotation, position and bottom-layer (elephant's foot) compensation. Reports the layer count, print time, volume and weight. |
| `about` | The running version, the latest version on npm, and how to update. |

### Print files

For ELEGOO printers, Scion Slicer writes `.goo` files the way ELEGOO SatelLite does: the same layer encoding, header fields and print-time estimate, byte for byte, as its tests check against slices made with SatelLite. The design and the rules are in [docs/design/goo-writer.md](docs/design/goo-writer.md) and [corpus/satellite](corpus/satellite/saturn-4-ultra-16k/README.md). So far that covers the Saturn 4 Ultra 16K, with anti-aliasing off.

## How it relates to DragonFruit

- **`vendor/DragonFruit`** is a git submodule pinned to an upstream commit. Its plugins (printer formats and material profiles) come with it as nested submodules.
- **`crates/`** is Scion Slicer's own code. It only *uses* the engine.
- **`patches/dragonfruit/`** holds engine changes that have to live inside DragonFruit. `scripts/bootstrap.sh` applies them to the pinned checkout. When a change is accepted upstream, its patch is deleted after the submodule moves past it. See [patches/README.md](patches/README.md).

## Build

Needs Rust (the toolchain is pinned in `rust-toolchain.toml`), Git and Node.

```bash
git clone --recurse-submodules https://github.com/scion-slicer/scion-slicer.git
cd scion-slicer
./scripts/bootstrap.sh
cargo build --release
```

`bootstrap.sh` checks out the submodules, applies the engine patches and generates DragonFruit's plugin encoder list. It is safe to run again; `--reset` starts the engine checkout over from the pinned commit.

## Install

Scion Slicer comes as an agent plugin: the MCP server plus skills that tell the agent how to check a print, explain the results and slice it. One plugin works in Claude Code, GitHub Copilot, Codex and Cursor. The server comes from npm, so you need [Node.js](https://nodejs.org) 18 or later. Prebuilt servers are published for macOS (Apple Silicon and Intel), Linux (x64 and arm64) and Windows (x64).

**Claude Code**

```
/plugin install scion-slicer --marketplace scion-slicer/scion-slicer
```

Claude Code does not update plugins from third-party marketplaces on its own. To get updates automatically, open `/plugin`, go to Marketplaces, select scion-slicer and turn on auto-update.

**GitHub Copilot CLI**

```bash
copilot plugin marketplace add scion-slicer/scion-slicer
```

```bash
copilot plugin install scion-slicer@scion-slicer
```

**Codex**

```bash
codex plugin marketplace add scion-slicer/scion-slicer
```

```bash
codex plugin add scion-slicer@scion-slicer
```

**Cursor:** import `https://github.com/scion-slicer/scion-slicer` as a team marketplace, then install scion-slicer from it.

**Any other MCP client:** run the server directly. It speaks MCP over stdio.

```bash
npx -y @scion-slicer/mcp@latest
```

### Updates

Once a day the server's launcher asks npm for the latest version number. Nothing about you or your files is sent. When a newer version exists, the server says so once per session, with the update command for your agent, and the `about` tool shows it too. Set `SCION_SLICER_NO_UPDATE_CHECK=1` to turn this off. Changes are listed in the [CHANGELOG](CHANGELOG.md).

## Use a source build

```bash
claude mcp add scion-slicer -- /path/to/scion-slicer/target/release/scion-slicer-mcp
```

A source build run directly never checks for updates.

The plugin's design, including what each agent reads and how releases work, is in [docs/design/agent-plugin.md](docs/design/agent-plugin.md).

## Releasing

```bash
node scripts/release.mjs 0.2.0
```

That writes the version into every file that carries it. Add a `## 0.2.0` section to [CHANGELOG.md](CHANGELOG.md), run `node scripts/release.mjs --check`, commit, and push a `v0.2.0` tag. The release workflow builds every platform in `npm/platforms.json`, publishes to npm and creates the GitHub release.

## Updating DragonFruit

```bash
git -C vendor/DragonFruit fetch origin
git -C vendor/DragonFruit checkout <new commit>
./scripts/bootstrap.sh --reset
cargo test --release
```

If a patch no longer applies, refresh it against the new commit (or delete it if upstream now includes the change), then commit the new submodule pointer together with the patches.

## License

AGPL-3.0-or-later, the same family as DragonFruit (AGPL-3.0). DragonFruit's plugins and material profiles are MIT-licensed by the Open Resin Alliance. See [NOTICE](NOTICE).
