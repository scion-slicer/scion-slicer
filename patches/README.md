# Engine patches

Changes to DragonFruit that have to live inside the engine, applied in file-name order by
`scripts/bootstrap.sh` to the pinned `vendor/DragonFruit` checkout. Anything that can be
built on the engine's public API belongs in `crates/` instead.

| Patches | What | Upstream |
|---|---|---|
| `dragonfruit/0001`–`0003` | Bottom-layer contour compensation (elephant's foot), RLE-native | [Open-Resin-Alliance/DragonFruit#1016](https://github.com/Open-Resin-Alliance/DragonFruit/pull/1016) |

## Lifecycle

1. Make the change on a branch of a DragonFruit fork, based on the pinned commit.
2. Export it here: `git format-patch --no-signature --zero-commit -o patches/dragonfruit <pinned>..<branch>`
   (number the files after the existing ones).
3. Optionally open a pull request upstream from that branch, and link it in the table.
4. When upstream merges it, move `vendor/DragonFruit` to a commit that includes it,
   delete its patch files and the table row, and run `scripts/bootstrap.sh --reset`.
