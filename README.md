# Scion Slicer

An open, agent-driven resin slicer, built on the [DragonFruit](https://github.com/Open-Resin-Alliance/DragonFruit) slicing engine by the [Open Resin Alliance](https://github.com/Open-Resin-Alliance).

Scion Slicer exposes resin printing tools over the [Model Context Protocol](https://modelcontextprotocol.io), so an AI agent can check and prepare prints. It uses DragonFruit's engine, printer plugins and material profiles as they are, and adds its own tools on top.

## Tools

| Tool | What it does |
|---|---|
| `check_print` | Checks a binary STL in print orientation for resin traps, suction cups, peel problems and ledges. Each finding has its layer range, location, size, cause and a suggested action. |

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

## Use it from an MCP client

The server speaks MCP over stdio. For Claude Code:

```bash
claude mcp add scion-slicer -- /path/to/scion-slicer/target/release/scion-slicer-mcp
```

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
