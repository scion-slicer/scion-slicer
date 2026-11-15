#!/usr/bin/env node
// Scion Slicer MCP launcher.
//
// Runs the native server binary from the platform package npm installed next to this
// one (@scion-slicer/mcp-<os>-<cpu>), with stdio passed straight through. It writes
// nothing to stdout: that belongs to the MCP connection.
//
// While the server runs, it also checks npm for a newer version, at most once a day,
// and writes the answer to a small file the server reads (SCION_SLICER_UPDATE_FILE).
// Set SCION_SLICER_NO_UPDATE_CHECK=1 to turn that off. It is also off when CI is set.

'use strict';

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const PACKAGE = require('../package.json');
const DAY_MS = 24 * 60 * 60 * 1000;
const FETCH_TIMEOUT_MS = 2000;
const VERSION_RE = /^\d+\.\d+\.\d+$/;

function fail(message) {
  process.stderr.write(`scion-slicer: ${message}\n`);
  process.exit(1);
}

function findBinary() {
  if (process.env.SCION_SLICER_BINARY) return process.env.SCION_SLICER_BINARY;
  const key = `${process.platform}-${process.arch}`;
  const name = `${PACKAGE.name}-${key}`;
  const exe = process.platform === 'win32' ? 'scion-slicer-mcp.exe' : 'scion-slicer-mcp';
  let dir;
  try {
    dir = path.dirname(require.resolve(`${name}/package.json`));
  } catch {
    const known = Object.keys(PACKAGE.optionalDependencies || {});
    if (!known.includes(name)) {
      fail(
        `there is no build for ${key} yet. Build from source: ` +
          'https://github.com/scion-slicer/scion-slicer#build',
      );
    }
    fail(
      `${name} is not installed. It is an optional dependency, so it is skipped when ` +
        'optional dependencies are turned off (for example --omit=optional). Reinstall with ' +
        'them on, or set SCION_SLICER_BINARY to a server binary.',
    );
  }
  const binary = path.join(dir, 'bin', exe);
  if (process.platform !== 'win32') {
    // Some package managers drop the executable bit on files that are not npm "bin" entries.
    try {
      fs.accessSync(binary, fs.constants.X_OK);
    } catch {
      try {
        fs.chmodSync(binary, 0o755);
      } catch {
        // Spawning will report the real problem.
      }
    }
  }
  return binary;
}

function updateCheckEnabled() {
  const optOut = process.env.SCION_SLICER_NO_UPDATE_CHECK;
  if (optOut && optOut !== '0') return false;
  if (process.env.CI) return false;
  return true;
}

function cacheDir() {
  if (process.env.SCION_SLICER_CACHE_DIR) return process.env.SCION_SLICER_CACHE_DIR;
  const home = os.homedir();
  if (process.platform === 'darwin') return path.join(home, 'Library', 'Caches', 'scion-slicer');
  if (process.platform === 'win32') {
    return path.join(process.env.LOCALAPPDATA || path.join(home, 'AppData', 'Local'), 'scion-slicer');
  }
  return path.join(process.env.XDG_CACHE_HOME || path.join(home, '.cache'), 'scion-slicer');
}

function readUpdateFile(file) {
  try {
    const data = JSON.parse(fs.readFileSync(file, 'utf8'));
    return data && typeof data === 'object' ? data : {};
  } catch {
    return {};
  }
}

function writeUpdateFile(file, data) {
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const tmp = `${file}.${process.pid}.tmp`;
    fs.writeFileSync(tmp, `${JSON.stringify(data)}\n`);
    fs.renameSync(tmp, file);
  } catch {
    // A cache we cannot write only means we check again next time.
  }
}

// Asks npm for the latest version number. Plain GET, nothing about the user is sent.
async function checkForUpdate(file) {
  const previous = readUpdateFile(file);
  const checkedAt = Date.parse(previous.checked_at || '');
  if (Number.isFinite(checkedAt) && Date.now() - checkedAt < DAY_MS) return;
  if (typeof fetch !== 'function') return;

  const registry = (process.env.SCION_SLICER_REGISTRY || 'https://registry.npmjs.org').replace(/\/+$/, '');
  let latest = VERSION_RE.test(previous.latest || '') ? previous.latest : null;
  try {
    const response = await fetch(`${registry}/${PACKAGE.name}/latest`, {
      headers: { accept: 'application/json' },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
    if (response.ok) {
      const body = await response.json();
      if (body && VERSION_RE.test(body.version || '')) latest = body.version;
    }
  } catch {
    // Offline, slow or blocked: stay quiet and try again tomorrow.
  }
  writeUpdateFile(file, { checked_at: new Date().toISOString(), latest });
}

function main() {
  const binary = findBinary();
  const env = { ...process.env };
  let updateFile = null;
  if (updateCheckEnabled()) {
    updateFile = path.join(cacheDir(), 'update-check.json');
    env.SCION_SLICER_UPDATE_FILE = updateFile;
  }

  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', env });
  child.on('error', (err) => fail(`could not start ${binary}: ${err.message}`));
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    process.on(signal, () => {
      if (!child.killed) child.kill(signal);
    });
  }
  child.on('exit', (code, signal) => {
    if (signal) {
      process.removeAllListeners(signal);
      process.kill(process.pid, signal);
    } else {
      process.exit(code ?? 1);
    }
  });

  if (updateFile) checkForUpdate(updateFile).catch(() => {});
}

main();
