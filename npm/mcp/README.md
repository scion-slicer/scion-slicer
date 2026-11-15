# @scion-slicer/mcp

The [Scion Slicer](https://github.com/scion-slicer/scion-slicer) MCP server: resin 3D printing tools for AI agents, built on the DragonFruit slicing engine.

```bash
npx -y @scion-slicer/mcp
```

The server speaks MCP over stdio. Most people get it through the Scion Slicer agent plugin, which also brings its skills; see the [README](https://github.com/scion-slicer/scion-slicer#readme).

This package is a small launcher. npm installs the server binary for your platform from one of the `@scion-slicer/mcp-<os>-<cpu>` packages.

## Update check

At most once a day the launcher asks the npm registry for the latest version number. Nothing about you or your files is sent. When a newer version exists, the server mentions it once per session. Set `SCION_SLICER_NO_UPDATE_CHECK=1` to turn this off; it is also off when `CI` is set.

## License

AGPL-3.0-or-later.
