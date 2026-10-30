// Builds the UwUPulse OBS plugin for one platform:
//
//   pnpm build:obs [--platform macos|windows-x64|windows-arm64|linux]
//                  [--deps <dir>] [--out <dir>] [--skip-ui]
//
// 1. the OBS SDK (libobs, obs-frontend-api, Qt 6, obs-browser's panel header)
//    into --deps (default ~/.cache/uwupulse-obs/deps, or UWUPULSE_OBS_DEPS),
//    once per buildspec.json;
// 2. the frontend (apps/desktop/dist), unless --skip-ui;
// 3. the Rust core (crates/uwupulse-obs) as a static library, on macOS for
//    arm64 and x86_64 joined with lipo;
// 4. the plugin with CMake, installed into --out (default
//    target/obs/<platform>) in the layout OBS loads:
//
//    macOS          uwupulse.plugin/Contents/{MacOS/uwupulse, Resources/{locale,ui}}
//    Windows/Linux  uwupulse/bin/64bit/uwupulse.{dll,so}, uwupulse/data/{locale,ui}

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const plugin = join(dirname(fileURLToPath(import.meta.url)), '..');
const root = join(plugin, '..', '..');
const cmake = process.env.CMAKE || 'cmake';

const fail = (message) => {
  console.error(`\n✗ ${message}`);
  process.exit(1);
};

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? undefined : process.argv[i + 1];
};

function hostPlatform() {
  if (process.platform === 'darwin') return 'macos';
  if (process.platform === 'linux') return 'linux';
  if (process.platform === 'win32')
    return process.arch === 'arm64' ? 'windows-arm64' : 'windows-x64';
  fail(`no OBS plugin build for ${process.platform}`);
}

const platform = arg('--platform') ?? hostPlatform();
const RUST_TARGETS = {
  macos: ['aarch64-apple-darwin', 'x86_64-apple-darwin'],
  'windows-x64': ['x86_64-pc-windows-msvc'],
  'windows-arm64': ['aarch64-pc-windows-msvc'],
  linux: ['x86_64-unknown-linux-gnu'],
};
if (!RUST_TARGETS[platform]) fail(`unknown platform ${platform}`);
const deps = resolve(
  arg('--deps') ??
    process.env.UWUPULSE_OBS_DEPS ??
    join(homedir(), '.cache', 'uwupulse-obs', 'deps'),
);
const out = resolve(arg('--out') ?? join(root, 'target', 'obs', platform));
const buildDir = join(root, 'target', 'obs', `build-${platform}`);
// What the core is built for: as far back as OBS 31 goes on macOS.
const env = { ...process.env, MACOSX_DEPLOYMENT_TARGET: '12.0' };

function run(cmd, args, opts = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', env, ...opts });
  if (r.status !== 0) fail(`${cmd} failed`);
  return r;
}

function capture(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: root, env, encoding: 'utf8', maxBuffer: 256 << 20 });
  if (r.status !== 0) {
    process.stderr.write(r.stderr ?? '');
    fail(`${cmd} ${args.join(' ')} failed`);
  }
  return r.stdout;
}

// 1. SDK
run(cmake, [
  `-DPLATFORM=${platform}`,
  `-DDEPS_DIR=${deps}`,
  '-P',
  join(plugin, 'cmake', 'deps.cmake'),
]);
const prefixPath = readFileSync(join(deps, 'prefix-path.txt'), 'utf8').trim();

// 2. Frontend
const ui = join(root, 'apps', 'desktop', 'dist');
if (!process.argv.includes('--skip-ui')) {
  run('pnpm', ['--filter', '@uwupulse/desktop', 'build'], { shell: process.platform === 'win32' });
}
if (!existsSync(join(ui, 'index.html'))) fail(`no frontend in ${ui}`);

// 3. Rust core
const host = /host: (\S+)/.exec(capture('rustc', ['-vV']))[1];

// Builds the static library for one target; its path and rustc's list of
// the system libraries it needs.
function buildCore(target) {
  const args = ['rustc', '-p', 'uwupulse-obs', '--lib', '--release', '--locked'];
  // The host's build shares target/release with the workspace.
  if (target !== host) args.push('--target', target);
  // No Bluetooth on macOS: OBS.app may not use it (see src/plugin.cpp).
  if (target.endsWith('-apple-darwin')) args.push('--no-default-features');
  args.push('--crate-type', 'staticlib', '--message-format=json');
  args.push('--', '--print', 'native-static-libs');
  console.log(`\n$ cargo ${args.join(' ')}`);
  const r = spawnSync('cargo', args, {
    cwd: root,
    env,
    encoding: 'utf8',
    maxBuffer: 256 << 20,
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  if (r.status !== 0) fail('cargo failed');
  let native;
  let lib;
  for (const line of r.stdout.split('\n')) {
    if (!line.startsWith('{')) continue;
    const msg = JSON.parse(line);
    const text = msg.reason === 'compiler-message' ? msg.message?.message : undefined;
    if (msg.reason === 'compiler-message' && msg.message?.rendered) {
      process.stderr.write(msg.message.rendered);
    }
    if (text?.startsWith('native-static-libs: ')) native = text.slice(20).trim();
    if (msg.reason === 'compiler-artifact' && msg.target?.name === 'uwupulse_obs') {
      lib = msg.filenames.find((f) => /\.(a|lib)$/.test(f));
    }
  }
  if (!lib) fail(`cargo built no static library for ${target}`);
  if (native === undefined) fail(`rustc printed no native-static-libs for ${target}`);
  return { lib, native: native.split(/\s+/).filter(Boolean) };
}

// rustc's list as CMake link items.
function linkItems(native, target) {
  const items = [];
  for (let i = 0; i < native.length; i++) {
    const item = native[i];
    // The CRT (CMake picks it) and libSystem (always linked on macOS).
    if (item.startsWith('/defaultlib:') || item === '-lSystem') continue;
    if (item === '-framework') items.push(`-Wl,-framework,${native[++i]}`);
    else items.push(item);
  }
  if (!target.endsWith('-windows-msvc')) return items;
  // windows-targets' import libraries (windows.0.52.0.lib) come with the
  // crate, which tells the linker where only when Cargo links.
  const meta = JSON.parse(
    capture('cargo', [
      'metadata',
      '--format-version',
      '1',
      '--locked',
      '--filter-platform',
      target,
    ]),
  );
  const libDirs = meta.packages
    .filter((p) => /^windows_.*_msvc$/.test(p.name))
    .map((p) => join(dirname(p.manifest_path), 'lib'))
    .filter(existsSync);
  return items.map((item) => {
    for (const dir of libDirs) {
      if (readdirSync(dir).includes(item)) return join(dir, item);
    }
    return item;
  });
}

const builds = RUST_TARGETS[platform].map((t) => ({ target: t, ...buildCore(t) }));
let coreLib = builds[0].lib;
if (builds.length > 1) {
  coreLib = join(root, 'target', 'obs', 'libuwupulse_obs-universal.a');
  mkdirSync(dirname(coreLib), { recursive: true });
  run('lipo', ['-create', ...builds.map((b) => b.lib), '-output', coreLib]);
}
const link = [...new Set(builds.flatMap((b) => linkItems(b.native, b.target)))];

// 4. Plugin
const configure = [
  '-S',
  plugin,
  '-B',
  buildDir,
  `-DCMAKE_PREFIX_PATH=${prefixPath}`,
  `-DOBS_BROWSER_INCLUDE=${join(deps, 'obs-browser')}`,
  `-DUWUPULSE_CORE_LIB=${coreLib}`,
  `-DUWUPULSE_CORE_LINK=${link.join(';')}`,
  `-DUWUPULSE_UI_DIR=${ui}`,
];
if (platform === 'macos') {
  configure.push(
    '-G',
    'Unix Makefiles',
    '-DCMAKE_BUILD_TYPE=Release',
    '-DCMAKE_OSX_ARCHITECTURES=arm64;x86_64',
    '-DCMAKE_OSX_DEPLOYMENT_TARGET=12.0',
  );
} else if (platform === 'linux') {
  configure.push('-G', 'Unix Makefiles', '-DCMAKE_BUILD_TYPE=Release');
} else {
  const arch = platform === 'windows-arm64' ? 'ARM64' : 'x64';
  // The newest Visual Studio there.
  configure.push('-A', arch);
}
run(cmake, configure);
run(cmake, ['--build', buildDir, '--config', 'Release', '--parallel']);
rmSync(out, { recursive: true, force: true });
run(cmake, ['--install', buildDir, '--config', 'Release', '--prefix', out]);
console.log(`\n✓ UwUPulse OBS plugin for ${platform} in ${out}`);
