#!/usr/bin/env node
// End-to-end test of the MCP server as an agent would run it.
//
//   node scripts/test-mcp.mjs <command> [args...]
//
// <command> starts the server through the npm launcher, for example
// `npx -y @scion-slicer/mcp@0.1.0` in a folder where the packages are installed, or
// `node npm/mcp/bin/scion-slicer-mcp.js` with SCION_SLICER_BINARY set.
//
// It writes a small STL with a sealed cavity, starts a fake npm registry that says a
// newer version exists, and checks over three sessions that:
// - the server starts, names itself and lists its tools;
// - check_print finds the resin trap;
// - the update note appears on the first tool result only, with the right command;
// - about reports the update;
// - list_printers, list_resins and slice work, and slice writes a V3.0 .goo file;
// - the launcher asks the registry once a day, not once per session;
// - run from the plugin (SCION_SLICER_PLUGIN_VERSION set), the note is about the plugin;
// - SCION_SLICER_NO_UPDATE_CHECK=1 turns all of it off.

import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const VERSION = JSON.parse(readFileSync(join(root, 'npm/mcp/package.json'), 'utf8')).version;
const FAKE_LATEST = '999.0.0';
const REQUEST_TIMEOUT_MS = 120_000;

const [command, ...commandArgs] = process.argv.slice(2);
if (!command) {
  console.error('usage: node scripts/test-mcp.mjs <command> [args...]');
  process.exit(2);
}

function check(condition, message) {
  if (!condition) throw new Error(message);
}

// A 10 x 10 x 6 mm box with a sealed 4 x 4 x 2 mm cavity, the same as the Rust test
// `sealed_cavity_in_a_mesh_is_a_resin_trap`. The cavity's faces point inward.
function writeTrapStl(file) {
  const triangles = [];
  const box = (lo, hi, inward) => {
    const corner = (i) => [i & 1 ? hi[0] : lo[0], i & 2 ? hi[1] : lo[1], i & 4 ? hi[2] : lo[2]];
    // Each face as four corners, counter-clockwise seen from outside.
    for (const face of [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]) {
      const q = face.map(corner);
      if (inward) q.reverse();
      triangles.push([q[0], q[1], q[2]], [q[0], q[2], q[3]]);
    }
  };
  box([0, 0, 0], [10, 10, 6], false);
  box([3, 3, 2], [7, 7, 4], true);

  const buf = Buffer.alloc(84 + triangles.length * 50);
  buf.write('scion-slicer test: box with a sealed cavity', 0, 'ascii');
  buf.writeUInt32LE(triangles.length, 80);
  triangles.forEach((tri, i) => {
    const at = 84 + i * 50 + 12; // the normal stays zero; the engine uses the winding
    tri.flat().forEach((v, j) => buf.writeFloatLE(v, at + j * 4));
  });
  writeFileSync(file, buf);
}

function startRegistry() {
  const hits = [];
  const server = createServer((req, res) => {
    hits.push(req.url);
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ name: '@scion-slicer/mcp', version: FAKE_LATEST }));
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({ url: `http://127.0.0.1:${server.address().port}`, hits, close: () => server.close() });
    });
  });
}

class Session {
  constructor(env) {
    const childEnv = { ...process.env, ...env };
    delete childEnv.CI; // the launcher skips the update check under CI
    // On Windows, npx is npx.cmd, which only runs through a shell.
    const windows = process.platform === 'win32';
    const quote = (a) => (/[\s"]/.test(a) ? `"${a.replace(/"/g, '\\"')}"` : a);
    this.child = windows
      ? spawn([command, ...commandArgs].map(quote).join(' '), { env: childEnv, shell: true })
      : spawn(command, commandArgs, { env: childEnv });
    this.stderr = '';
    this.pending = new Map();
    this.nextId = 1;
    let buffer = '';
    this.child.stdout.setEncoding('utf8');
    this.child.stdout.on('data', (chunk) => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        const message = JSON.parse(line);
        const waiter = this.pending.get(message.id);
        if (waiter) {
          this.pending.delete(message.id);
          waiter(message);
        }
      }
    });
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', (chunk) => {
      this.stderr += chunk;
    });
    this.exited = new Promise((resolve) => this.child.on('exit', (code) => resolve(code)));
  }

  request(method, params) {
    const id = this.nextId++;
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${method} timed out\n${this.stderr}`)), REQUEST_TIMEOUT_MS);
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        if (message.error) reject(new Error(`${method}: ${JSON.stringify(message.error)}`));
        else resolve(message.result);
      });
    });
  }

  notify(method, params = {}) {
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, params })}\n`);
  }

  async start(clientName) {
    const init = await this.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: clientName, version: '1.0.0' },
    });
    this.notify('notifications/initialized');
    return init;
  }

  callTool(name, args = {}) {
    return this.request('tools/call', { name, arguments: args });
  }

  async close() {
    this.child.stdin.end();
    const code = await Promise.race([this.exited, new Promise((r) => setTimeout(() => r('timeout'), 15_000))]);
    check(code === 0, `server did not exit cleanly (${code})\n${this.stderr}`);
  }
}

async function waitFor(condition, ms, what) {
  const until = Date.now() + ms;
  while (!condition()) {
    check(Date.now() < until, `timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

async function main() {
  const tmp = mkdtempSync(join(tmpdir(), 'scion-slicer-test-'));
  const stl = join(tmp, 'trap.stl');
  writeTrapStl(stl);
  const registry = await startRegistry();
  const cache = join(tmp, 'cache');
  const env = { SCION_SLICER_REGISTRY: registry.url, SCION_SLICER_CACHE_DIR: cache };

  try {
    // 1. First session: the launcher checks the registry, the note appears once.
    let s = new Session(env);
    const init = await s.start('claude-code');
    check(init.serverInfo.name === 'scion-slicer', `server calls itself ${init.serverInfo.name}`);
    check(init.serverInfo.version === VERSION, `server is ${init.serverInfo.version}, package is ${VERSION}`);
    const tools = (await s.request('tools/list', {})).tools.map((t) => t.name);
    for (const t of ['check_print', 'about', 'list_printers', 'list_resins', 'slice']) {
      check(tools.includes(t), `no ${t} tool: ${tools}`);
    }

    await waitFor(() => existsSync(join(cache, 'update-check.json')), 10_000, 'the update check');
    const first = await s.callTool('check_print', { stl_path: stl });
    check(!first.isError, `check_print failed: ${JSON.stringify(first.content)}`);
    const report = JSON.parse(first.content[0].text);
    const traps = report.findings.filter((f) => f.kind === 'resin_trap');
    check(traps.length === 1, `expected one resin trap, got ${JSON.stringify(report.findings)}`);
    check(first.content.length === 2, 'no update note on the first tool result');
    const note = first.content[1].text;
    check(note.includes(FAKE_LATEST) && note.includes('claude plugin update'), `note: ${note}`);

    const second = await s.callTool('check_print', { stl_path: stl });
    check(second.content.length === 1, 'update note repeated on the second tool result');
    const about = JSON.parse((await s.callTool('about')).content[0].text);
    check(about.update_available === true && about.latest_known === FAKE_LATEST, `about: ${JSON.stringify(about)}`);

    const printers = JSON.parse((await s.callTool('list_printers')).content[0].text);
    check(printers.printers.length > 0, `list_printers: ${JSON.stringify(printers)}`);
    const resins = JSON.parse((await s.callTool('list_resins')).content[0].text);
    check(resins.resins.length > 0, `list_resins: ${JSON.stringify(resins).slice(0, 500)}`);
    const goo = join(tmp, 'trap.goo');
    const sliced = await s.callTool('slice', { stl_path: stl, output_path: goo, resin: resins.resins[0].name });
    check(!sliced.isError, `slice failed: ${JSON.stringify(sliced.content)}`);
    const sliceReport = JSON.parse(sliced.content[0].text);
    check(sliceReport.layers === 120 && sliceReport.print_time_s > 0, `slice: ${sliced.content[0].text}`);
    check(readFileSync(goo).subarray(0, 4).toString() === 'V3.0', 'the .goo file does not start with V3.0');
    const refused = await s.callTool('slice', { stl_path: stl, output_path: goo });
    check(refused.isError, 'slice replaced an existing file without overwrite');
    await s.close();
    check(registry.hits.length === 1, `registry asked ${registry.hits.length} times`);

    // 2. Second session the same day, run from the plugin: no new request, and the note
    //    is about the plugin's version.
    s = new Session({ ...env, SCION_SLICER_PLUGIN_VERSION: '0.0.1' });
    await s.start('codex-mcp-client');
    const again = await s.callTool('check_print', { stl_path: stl });
    check(again.content.length === 2, 'no update note in the second session');
    const pluginNote = again.content[1].text;
    check(
      pluginNote.includes(`plugin ${FAKE_LATEST} is available (you have 0.0.1)`) && pluginNote.includes('codex'),
      `plugin note: ${pluginNote}`,
    );
    const aboutPlugin = JSON.parse((await s.callTool('about')).content[0].text);
    check(aboutPlugin.plugin_version === '0.0.1', `about: ${JSON.stringify(aboutPlugin)}`);
    await s.close();
    check(registry.hits.length === 1, `registry asked again the same day (${registry.hits.length} times)`);

    // 3. Opted out: no request, no note.
    s = new Session({ ...env, SCION_SLICER_CACHE_DIR: join(tmp, 'cache-off'), SCION_SLICER_NO_UPDATE_CHECK: '1' });
    await s.start('claude-code');
    const quiet = await s.callTool('check_print', { stl_path: stl });
    check(quiet.content.length === 1, 'update note shown while opted out');
    const aboutOff = JSON.parse((await s.callTool('about')).content[0].text);
    check(aboutOff.update_available === false && aboutOff.update_check.startsWith('off'), `about: ${JSON.stringify(aboutOff)}`);
    await s.close();
    check(registry.hits.length === 1, 'registry asked while opted out');
    check(!existsSync(join(tmp, 'cache-off')), 'cache written while opted out');

    console.log(
      `MCP session test passed: scion-slicer ${VERSION} on ${process.platform}-${process.arch}, ` +
        `${report.layers} layers, ${report.findings.length} findings, one resin trap; ` +
        `sliced with ${sliceReport.resin} into ${sliceReport.layers} layers.`,
    );
  } finally {
    registry.close();
    rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error(`MCP session test failed: ${err.message}`);
  process.exit(1);
});
