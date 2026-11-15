#!/usr/bin/env node
// Builds one platform package (@scion-slicer/mcp-<os>-<cpu>) around a server binary.
//
//   node scripts/npm-platform.mjs <platform> <binary> <out-dir>
//
// <platform> is an "npm" key from npm/platforms.json, for example darwin-arm64.

import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const [key, binary, outDir] = process.argv.slice(2);
if (!key || !binary || !outDir) {
  console.error('usage: node scripts/npm-platform.mjs <platform> <binary> <out-dir>');
  process.exit(2);
}

const platforms = JSON.parse(readFileSync(join(root, 'npm/platforms.json'), 'utf8'));
const platform = platforms.find((p) => p.npm === key);
if (!platform) {
  console.error(`unknown platform ${key}; known: ${platforms.map((p) => p.npm).join(', ')}`);
  process.exit(2);
}
const launcher = JSON.parse(readFileSync(join(root, 'npm/mcp/package.json'), 'utf8'));
const exe = platform.os === 'win32' ? 'scion-slicer-mcp.exe' : 'scion-slicer-mcp';

rmSync(outDir, { recursive: true, force: true });
mkdirSync(join(outDir, 'bin'), { recursive: true });
copyFileSync(binary, join(outDir, 'bin', exe));
if (platform.os !== 'win32') chmodSync(join(outDir, 'bin', exe), 0o755);
copyFileSync(join(root, 'LICENSE'), join(outDir, 'LICENSE'));

const pkg = {
  name: `${launcher.name}-${key}`,
  version: launcher.version,
  description: `The Scion Slicer MCP server binary for ${key}. Install @scion-slicer/mcp instead.`,
  license: launcher.license,
  homepage: launcher.homepage,
  repository: launcher.repository,
  os: [platform.os],
  cpu: [platform.cpu],
  ...(platform.libc ? { libc: [platform.libc] } : {}),
  files: ['bin', 'README.md'],
  preferUnplugged: true,
};
writeFileSync(join(outDir, 'package.json'), `${JSON.stringify(pkg, null, 2)}\n`);
writeFileSync(
  join(outDir, 'README.md'),
  `# ${pkg.name}\n\nThe Scion Slicer MCP server binary for ${key}. You do not need to install this ` +
    'yourself: install [@scion-slicer/mcp](https://www.npmjs.com/package/@scion-slicer/mcp), which ' +
    'picks the right binary for your platform.\n',
);
console.log(`${pkg.name}@${pkg.version} -> ${outDir}`);
