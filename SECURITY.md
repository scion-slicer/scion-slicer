# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue or pull request.

**[Report a vulnerability](https://github.com/scion-slicer/scion-slicer/security/advisories/new)** (GitHub's private vulnerability reporting). Only the maintainers can see your report, and we can work on a fix with you there before anything is public.

A useful report says:

- what you found and what an attacker could do with it;
- the version (`about` reports it, or `npx -y @scion-slicer/mcp --version`) and your platform;
- how to reproduce it: steps, a file that triggers it, or a proof of concept.

We aim to acknowledge a report within 7 days and to keep you updated until it is fixed. Once a fix is released, we publish an advisory and credit you, unless you would rather not be named.

## What is in scope

- The MCP server (`crates/`) and the npm launcher (`npm/mcp`).
- The npm packages `@scion-slicer/mcp` and `@scion-slicer/mcp-<platform>`.
- The agent plugin (`plugin/`), the release workflows (`.github/workflows/`) and the scripts that build and publish releases (`scripts/`).

Examples: a crafted STL that crashes the server or makes it use unbounded memory, a tool reading or writing files it should not, the update check sending data it should not, or a way to publish packages without the maintainers.

The slicing engine itself comes from [DragonFruit](https://github.com/Open-Resin-Alliance/DragonFruit). If a problem is in DragonFruit's own code, please report it to the Open Resin Alliance as well; we will help coordinate.

## Supported versions

Security fixes go into the latest release. Update to it with your agent's plugin update command, or see `about`.

## How releases are protected

- npm packages are published only from this repository's release workflow, through npm trusted publishing (no stored tokens), from version 0.1.1 on. Packages published that way carry a provenance statement linking them to the workflow run that built them.
- A release runs only from a `v*` tag, which only maintainers can create, and waits for a maintainer's approval before publishing.
- The update check sends one plain request for the latest version number to the npm registry and nothing else. `SCION_SLICER_NO_UPDATE_CHECK=1` turns it off.
