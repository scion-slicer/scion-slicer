#!/usr/bin/env node
// One version everywhere.
//
//   node scripts/release.mjs X.Y.Z            write X.Y.Z into every file that carries it
//   node scripts/release.mjs --check          check that every file agrees
//   node scripts/release.mjs --check vX.Y.Z   ...and that they agree with this tag
//
// Files: Cargo.toml (workspace), Cargo.lock (our crates), npm/mcp/package.json (its
// version and the platform packages it depends on), both plugin manifests, and the plugin
// version passed to the server in both MCP files. CHANGELOG.md must have a "## X.Y.Z"
// section. The marketplace entry has no version: the plugin manifests carry it.
//
// The MCP files run `@scion-slicer/mcp@latest`, never a pinned version: the plugin is
// served from main as soon as it is pushed, before the release is on npm, and a pinned
// version would not exist yet.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const VERSION_RE = /^\d+\.\d+\.\d+$/;
const PLUGIN = 'plugin/scion-slicer';
const MANIFESTS = [`${PLUGIN}/plugin.json`, `${PLUGIN}/.claude-plugin/plugin.json`];
const MCP_FILES = [`${PLUGIN}/mcp.json`, `${PLUGIN}/.mcp.json`];
const CRATES = ['scion-slicer-core', 'scion-slicer-goo', 'scion-slicer-mcp'];
const SERVER_PACKAGE = '@scion-slicer/mcp';
const PLUGIN_VERSION_ENV = 'SCION_SLICER_PLUGIN_VERSION';

const read = (file) => readFileSync(join(root, file), 'utf8');
const readJson = (file) => JSON.parse(read(file));
const writeJson = (file, data) => writeFileSync(join(root, file), `${JSON.stringify(data, null, 2)}\n`);

function cargoVersion() {
  const section = read('Cargo.toml').match(/\[workspace\.package\][^[]*/);
  return section?.[0].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
}

function lockVersion(crate) {
  const re = new RegExp(`name = "${crate}"\\nversion = "([^"]+)"`);
  return read('Cargo.lock').match(re)?.[1];
}

function server(file) {
  return readJson(file).mcpServers['scion-slicer'];
}

function platformPackages(version) {
  const platforms = readJson('npm/platforms.json');
  return Object.fromEntries(platforms.map((p) => [`${SERVER_PACKAGE}-${p.npm}`, version]));
}

function check(tag) {
  const problems = [];
  const version = cargoVersion();
  if (!version || !VERSION_RE.test(version)) problems.push(`Cargo.toml: no X.Y.Z workspace version`);
  const expect = (where, actual) => {
    if (actual !== version) problems.push(`${where}: ${actual ?? 'missing'}, expected ${version}`);
  };

  if (tag !== undefined && tag !== `v${version}`) problems.push(`tag ${tag} does not match v${version}`);
  for (const crate of CRATES) expect(`Cargo.lock ${crate}`, lockVersion(crate));

  const launcher = readJson('npm/mcp/package.json');
  expect('npm/mcp/package.json version', launcher.version);
  const wanted = platformPackages(version);
  if (JSON.stringify(launcher.optionalDependencies) !== JSON.stringify(wanted)) {
    problems.push('npm/mcp/package.json optionalDependencies do not match npm/platforms.json at this version');
  }

  const [agent, claude] = MANIFESTS.map(readJson);
  for (const [file, manifest] of [[MANIFESTS[0], agent], [MANIFESTS[1], claude]]) {
    expect(`${file} version`, manifest.version);
  }
  for (const field of ['name', 'description', 'author', 'homepage', 'repository', 'license', 'keywords']) {
    if (JSON.stringify(agent[field]) !== JSON.stringify(claude[field])) {
      problems.push(`plugin manifests disagree on ${field}`);
    }
  }

  const servers = MCP_FILES.map(server);
  if (JSON.stringify(servers[0]) !== JSON.stringify(servers[1])) problems.push('MCP files start the server differently');
  for (const [i, file] of MCP_FILES.entries()) {
    if (!servers[i].args.includes(`${SERVER_PACKAGE}@latest`)) {
      problems.push(`${file}: must run ${SERVER_PACKAGE}@latest, not a pinned version`);
    }
    expect(`${file} ${PLUGIN_VERSION_ENV}`, servers[i].env?.[PLUGIN_VERSION_ENV]);
  }

  const marketplace = readJson('.claude-plugin/marketplace.json');
  const entry = marketplace.plugins.find((p) => p.name === agent.name);
  if (!entry) problems.push(`marketplace has no ${agent.name} entry`);
  else if ('version' in entry) problems.push('marketplace entry has a version; keep it in the plugin manifests only');
  if (entry && entry.description !== agent.description) problems.push('marketplace description differs from the plugin manifest');

  if (!new RegExp(`^## ${version.replace(/\./g, '\\.')}\\b`, 'm').test(read('CHANGELOG.md'))) {
    problems.push(`CHANGELOG.md has no "## ${version}" section`);
  }

  if (problems.length) {
    console.error(`Version check failed:\n${problems.map((p) => `  - ${p}`).join('\n')}`);
    process.exit(1);
  }
  console.log(`All files agree on ${version}.`);
}

function stamp(version) {
  const toml = read('Cargo.toml').replace(
    /(\[workspace\.package\][^[]*?^version\s*=\s*")[^"]+(")/m,
    `$1${version}$2`,
  );
  writeFileSync(join(root, 'Cargo.toml'), toml);

  let lock = read('Cargo.lock');
  for (const crate of CRATES) {
    lock = lock.replace(new RegExp(`(name = "${crate}"\\nversion = ")[^"]+(")`), `$1${version}$2`);
  }
  writeFileSync(join(root, 'Cargo.lock'), lock);

  const launcher = readJson('npm/mcp/package.json');
  launcher.version = version;
  launcher.optionalDependencies = platformPackages(version);
  writeJson('npm/mcp/package.json', launcher);

  for (const file of MANIFESTS) {
    const manifest = readJson(file);
    manifest.version = version;
    writeJson(file, manifest);
  }
  for (const file of MCP_FILES) {
    const config = readJson(file);
    const entry = config.mcpServers['scion-slicer'];
    entry.env = { ...entry.env, [PLUGIN_VERSION_ENV]: version };
    writeJson(file, config);
  }
  console.log(`Wrote ${version}. Next:
  1. Add a "## ${version}" section to CHANGELOG.md.
  2. node scripts/release.mjs --check
  3. Commit, then tag and push: git tag v${version} && git push origin v${version}`);
}

const args = process.argv.slice(2);
if (args[0] === '--check') {
  check(args[1]);
} else if (args.length === 1 && VERSION_RE.test(args[0])) {
  stamp(args[0]);
} else {
  console.error('usage: node scripts/release.mjs X.Y.Z | --check [vX.Y.Z]');
  process.exit(2);
}
