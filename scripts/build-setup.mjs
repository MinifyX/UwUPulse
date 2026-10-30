// Builds UwUPulse's downloads for the system it runs on: on Windows and macOS
// the app packed into the setup with Nyu in it, on Linux the app as .deb,
// .rpm and a portable folder.
//
//   pnpm build:setup                       for this machine
//   pnpm build:setup --target <triple>     another architecture (macOS:
//                                          universal-apple-darwin for both
//                                          Intel and Apple silicon, as the
//                                          release has it)
//
// Everything lands in target/installers/, under the names the release
// publishes — no version in them, so a link to the newest release's file
// stays the same forever:
//
//   Windows  UwUPulse-windows-x64-setup.exe           what people run, and what updates run
//            UwUPulse-windows-arm64-setup.exe         the same for ARM
//   macOS    UwUPulse-macos-universal.dmg             what people open
//            UwUPulse-update-macos-universal          the setup program inside it, for updates
//   Linux    UwUPulse-linux-<arch>.deb                the app for apt/dpkg, updates itself
//            UwUPulse-linux-<arch>.rpm                the app for dnf/zypper/rpm, updates itself
//            UwUPulse-linux-<arch>-portable.tar.gz    unpack and run, no updates
//            UwUPulse-update-linux-x64.AppImage       x64 only: the setup, for copies an
//                                                   earlier setup installed; not a download
//
// The setup also carries the OBS plugin (plugins/obs) everywhere but Linux on
// ARM, which is published on its own next to it as well:
//
//   Windows  UwUPulse-obs-windows-<arch>.zip          uwupulse/, for %ProgramData%\obs-studio\plugins
//   macOS    UwUPulse-obs-macos-universal.zip         uwupulse.plugin
//   Linux    UwUPulse-obs-linux-x64.tar.gz            uwupulse/, for ~/.config/obs-studio/plugins
//            UwUPulse-obs-linux-x64.deb               package uwupulse-obs, system-wide
//
// The plugin comes from UWUPULSE_OBS_PLUGIN (its folder as `pnpm build:obs`
// leaves it, or that folder as .tar.gz, as CI uploads it), else from
// target/obs/<platform>. Without it the setup is built without the plugin,
// unless UWUPULSE_OBS_PLUGIN was given. In CI UWUPULSE_OBS_ARTIFACT names the
// artifact of the OBS plugin job running alongside: the app is built first,
// then the plugin is waited for.
//
// Nothing here signs anything: `pnpm release` signs what the updater runs,
// under the versioned names installed apps expect, on the machine that holds
// the key. The builds below never see it.

import { execFileSync, execSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { setTimeout as sleep } from 'node:timers/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const run = (command, env = {}) =>
  execSync(command, { cwd: root, stdio: 'inherit', env: { ...process.env, ...env } });

// Keep the update-signing key out of the builds: they run hundreds of
// third-party build scripts (Cargo build.rs, npm) that would otherwise see it.
delete process.env.TAURI_SIGNING_PRIVATE_KEY;
delete process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
// CI's token, for the OBS plugin's artifact only: the same goes for it.
const githubToken = process.env.GITHUB_TOKEN;
delete process.env.GITHUB_TOKEN;
delete process.env.GH_TOKEN;

const fail = (message) => {
  console.error(`\n✗ ${message}`);
  process.exit(1);
};

const targetIndex = process.argv.indexOf('--target');
const target = targetIndex > 0 ? process.argv[targetIndex + 1] : undefined;
const targetArg = target ? ` --target ${target}` : '';

const { version } = JSON.parse(
  readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
);
{
  const setup = JSON.parse(
    readFileSync(join(root, 'apps/setup/src-tauri/tauri.conf.json'), 'utf8'),
  );
  if (setup.version !== version) fail(`apps/setup says ${setup.version}, the app says ${version}.`);
  // The desktop's front end shows the version from its package.json.
  for (const app of ['desktop', 'setup']) {
    const pkg = JSON.parse(readFileSync(join(root, `apps/${app}/package.json`), 'utf8'));
    if (pkg.version !== version) {
      fail(`apps/${app}/package.json says ${pkg.version}, tauri.conf.json says ${version}.`);
    }
  }
}

const release = join(root, 'target', ...(target ? [target] : []), 'release');
const bundles = join(release, 'bundle');
const out = join(root, 'target', 'installers');
mkdirSync(out, { recursive: true });

/** The architecture as release file names say it. */
function arch() {
  if (target === 'universal-apple-darwin') return 'universal';
  const name = target ?? `${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}-host`;
  return name.startsWith('aarch64') ? 'arm64' : 'x64';
}

/** A Tauri config override, as a file: quoting JSON on a command line is a trap on every shell. */
function configFile(name, config) {
  const path = join(mkdtempSync(join(tmpdir(), 'uwupulse-build-')), `${name}.json`);
  writeFileSync(path, JSON.stringify(config));
  return path;
}

function only(dir, test, what) {
  const found = existsSync(dir) ? readdirSync(dir).filter(test) : [];
  if (found.length !== 1) fail(`Expected one ${what} in ${dir}, found ${found.length}.`);
  return join(dir, found[0]);
}

const produced = [];

// The OBS plugin: its top-level folder, and the name of its CI build.
const obsTop = process.platform === 'darwin' ? 'uwupulse.plugin' : 'uwupulse';
const obsPlatform =
  process.platform === 'darwin'
    ? 'macos-universal'
    : process.platform === 'win32'
      ? `windows-${arch()}`
      : arch() === 'x64'
        ? 'linux-x64'
        : null;
// Windows' own bsdtar, which writes zip and reads both; Git's GNU tar may come first in PATH.
const tarExe =
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
    : 'tar';

/** A folder holding only the plugin, from a folder or a .tar.gz of one. */
function obsFolder(source) {
  let dir = source;
  if (source.endsWith('.tar.gz')) {
    dir = mkdtempSync(join(tmpdir(), 'uwupulse-obs-'));
    execFileSync(tarExe, ['-xzf', source, '-C', dir]);
  }
  const entries = existsSync(dir) ? readdirSync(dir).filter((n) => n !== '.DS_Store') : [];
  if (!entries.includes(obsTop)) return null;
  if (entries.length === 1) return dir;
  // Only the plugin goes into the setup.
  const only = mkdtempSync(join(tmpdir(), 'uwupulse-obs-'));
  cpSync(join(dir, obsTop), join(only, obsTop), { recursive: true, verbatimSymlinks: true });
  return only;
}

/**
 * CI: waits for the OBS plugin job of this run (obs-plugin.yml, called by
 * installers.yml) and fetches its artifact, a zip around the .tar.gz.
 */
async function fetchObsArtifact(name) {
  const repo = process.env.GITHUB_REPOSITORY;
  const run = process.env.GITHUB_RUN_ID;
  const attempt = process.env.GITHUB_RUN_ATTEMPT ?? '1';
  const token = githubToken;
  if (!repo || !run || !token)
    fail('UWUPULSE_OBS_ARTIFACT needs GITHUB_REPOSITORY, GITHUB_RUN_ID and GITHUB_TOKEN.');
  const headers = { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json' };
  const api = async (path) => {
    const response = await fetch(`https://api.github.com/repos/${repo}/${path}`, { headers });
    if (!response.ok) throw new Error(`${path}: ${response.status}`);
    return response.json();
  };
  const deadline = Date.now() + 50 * 60_000;
  console.log(`\n▸ Waiting for the OBS plugin (${name})`);
  for (;;) {
    try {
      const { artifacts } = await api(`actions/runs/${run}/artifacts?name=${name}`);
      const artifact = artifacts.find((a) => a.name === name && !a.expired);
      if (artifact) {
        const response = await fetch(artifact.archive_download_url, { headers });
        if (!response.ok) throw new Error(`download: ${response.status}`);
        const dir = mkdtempSync(join(tmpdir(), 'uwupulse-obs-artifact-'));
        const zip = join(dir, 'artifact.zip');
        writeFileSync(zip, Buffer.from(await response.arrayBuffer()));
        if (process.platform === 'linux') execFileSync('unzip', ['-q', zip, '-d', dir]);
        else execFileSync(tarExe, ['-xf', zip, '-C', dir]);
        const packed = join(dir, `${name}.tar.gz`);
        if (!existsSync(packed)) fail(`${name} holds no ${name}.tar.gz`);
        return packed;
      }
      const { jobs } = await api(`actions/runs/${run}/attempts/${attempt}/jobs?per_page=100`);
      const job = jobs.find((j) => j.name.endsWith(`Plugin (${obsPlatform})`));
      if (job?.status === 'completed' && job.conclusion !== 'success') {
        fail(`The OBS plugin job ended ${job.conclusion}: ${job.html_url}`);
      }
    } catch (error) {
      console.log(`  (asking again: ${error.message})`);
    }
    if (Date.now() > deadline) fail(`No ${name} after 50 minutes.`);
    await sleep(20_000);
  }
}

/** The plugin folder for the setup, or null for a setup without it. */
async function obsPlugin() {
  if (!obsPlatform) return null;
  const artifact = process.env.UWUPULSE_OBS_ARTIFACT;
  const given = artifact ? await fetchObsArtifact(artifact) : process.env.UWUPULSE_OBS_PLUGIN;
  const fallback = join(
    root,
    'target',
    'obs',
    obsPlatform === 'macos-universal'
      ? 'macos'
      : obsPlatform === 'linux-x64'
        ? 'linux'
        : obsPlatform,
  );
  const folder = obsFolder(given ?? fallback);
  if (folder) return folder;
  if (given) fail(`No ${obsTop} in ${given}.`);
  console.warn(`\n! No OBS plugin in ${fallback} (pnpm build:obs): the setup is built without it.`);
  return null;
}

/** The plugin on its own, for the release: a zip, as people unpack it on Windows and macOS. */
function publishObsZip(folder) {
  const zip = join(out, `UwUPulse-obs-${obsPlatform}.zip`);
  rmSync(zip, { force: true });
  if (process.platform === 'darwin') {
    execFileSync('ditto', ['-c', '-k', '--keepParent', join(folder, obsTop), zip]);
  } else {
    execFileSync(tarExe, ['-a', '-cf', zip, '-C', folder, obsTop]);
  }
  produced.push(zip);
}

/**
 * Linux: the plugin as a folder for ~/.config/obs-studio/plugins, and as
 * package `uwupulse-obs` in the system's paths, the way distributions ship
 * OBS plugins.
 */
function publishObsLinux(folder) {
  const portable = join(out, 'UwUPulse-obs-linux-x64.tar.gz');
  execFileSync('tar', [
    '--owner=0',
    '--group=0',
    '--numeric-owner',
    '-czf',
    portable,
    '-C',
    folder,
    obsTop,
  ]);
  const pkg = mkdtempSync(join(tmpdir(), 'uwupulse-obs-deb-'));
  const lib = join(pkg, 'usr/lib/x86_64-linux-gnu/obs-plugins');
  const share = join(pkg, 'usr/share/obs/obs-plugins');
  mkdirSync(lib, { recursive: true });
  mkdirSync(share, { recursive: true });
  mkdirSync(join(pkg, 'DEBIAN'));
  copyFileSync(join(folder, obsTop, 'bin/64bit/uwupulse.so'), join(lib, 'uwupulse.so'));
  chmodSync(join(lib, 'uwupulse.so'), 0o755);
  execFileSync('cp', ['-a', join(folder, obsTop, 'data'), join(share, 'uwupulse')]);
  const size = execFileSync('du', ['-sk', join(pkg, 'usr')], { encoding: 'utf8' }).split('\t')[0];
  writeFileSync(
    join(pkg, 'DEBIAN/control'),
    [
      'Package: uwupulse-obs',
      `Version: ${version}`,
      'Architecture: amd64',
      'Maintainer: UwUPulse <https://github.com/MinifyX/UwUPulse>',
      `Installed-Size: ${size}`,
      'Depends: obs-studio (>= 31), libdbus-1-3, libc6',
      'Section: video',
      'Priority: optional',
      'Homepage: https://github.com/MinifyX/UwUPulse',
      'Description: UwUPulse for OBS Studio',
      ' Heart rate from an Apple Watch or a Bluetooth chest strap, inside OBS',
      ' Studio: the UwUPulse dock and an overlay source. Everything stays local.',
      '',
    ].join('\n'),
  );
  const deb = join(out, 'UwUPulse-obs-linux-x64.deb');
  execFileSync('dpkg-deb', ['--root-owner-group', '-Zxz', '--build', pkg, deb], {
    stdio: 'inherit',
  });
  rmSync(pkg, { recursive: true, force: true });
  produced.push(portable, deb);
}

if (process.platform === 'win32') {
  console.log(`\n▸ Building UwUPulse ${version}`);
  run(`pnpm --filter @uwupulse/desktop tauri build --no-bundle${targetArg}`);
  const app = join(release, 'uwupulse-desktop.exe');
  if (!existsSync(app)) fail(`Missing ${app}`);

  const obs = await obsPlugin();
  console.log('\n▸ Packing it into the setup');
  run(`pnpm --filter @uwupulse/setup tauri build --no-bundle${targetArg}`, {
    UWUPULSE_SETUP_PAYLOAD: app,
    UWUPULSE_SETUP_OBS_PAYLOAD: obs ?? '',
  });
  if (obs) publishObsZip(obs);
  const setup = join(out, `UwUPulse-windows-${arch()}-setup.exe`);
  copyFileSync(join(release, 'uwupulse-setup.exe'), setup);
  produced.push(setup);
} else if (process.platform === 'darwin') {
  // Ad-hoc signed: no Apple developer ID, but a sealed bundle, which Apple
  // silicon insists on and which keeps the app intact through the setup.
  const macOS = { signingIdentity: '-', minimumSystemVersion: '11.0' };
  const apps = mkdtempSync(join(tmpdir(), 'uwupulse-apps-'));

  console.log(`\n▸ Building UwUPulse ${version}`);
  run(
    `pnpm --filter @uwupulse/desktop tauri build --bundles app${targetArg} --config "${configFile('app', { bundle: { macOS } })}"`,
  );
  execFileSync('ditto', [join(bundles, 'macos', 'UwUPulse.app'), join(apps, 'UwUPulse.app')]);

  const obs = await obsPlugin();
  console.log('\n▸ Packing it into the setup');
  const setupConfig = configFile('setup', {
    bundle: { active: true, targets: ['dmg'], macOS },
  });
  run(
    `pnpm --filter @uwupulse/setup tauri build --bundles dmg${targetArg} --config "${setupConfig}"`,
    {
      UWUPULSE_SETUP_PAYLOAD: apps,
      UWUPULSE_SETUP_OBS_PAYLOAD: obs ?? '',
    },
  );
  if (obs) publishObsZip(obs);
  const dmg = join(out, `UwUPulse-macos-${arch()}.dmg`);
  copyFileSync(
    only(join(bundles, 'dmg'), (name) => name.endsWith('.dmg'), 'disk image'),
    dmg,
  );
  const update = join(out, `UwUPulse-update-macos-${arch()}`);
  copyFileSync(join(release, 'uwupulse-setup'), update);
  produced.push(dmg, update);
  rmSync(apps, { recursive: true, force: true });
} else {
  // The AppImage, unpacked, is both the portable folder and what the setup
  // installs: it runs from its AppDir, brings its own WebKit and needs no FUSE.
  const apps = mkdtempSync(join(tmpdir(), 'uwupulse-apps-'));
  const unpackAppImage = (image, name) => {
    const work = mkdtempSync(join(tmpdir(), 'uwupulse-appimage-'));
    execFileSync('chmod', ['+x', image]);
    execFileSync(image, ['--appimage-extract'], { cwd: work, stdio: 'ignore' });
    renameSync(join(work, 'squashfs-root'), join(apps, name));
    rmSync(work, { recursive: true, force: true });
  };

  console.log(`\n▸ Building UwUPulse ${version}`);
  run(`pnpm --filter @uwupulse/desktop tauri build --bundles appimage${targetArg}`);
  unpackAppImage(
    only(join(bundles, 'appimage'), (name) => name.endsWith('.AppImage'), 'AppImage of UwUPulse'),
    'UwUPulse',
  );
  // The next build bundles into the same folders.
  rmSync(join(bundles, 'appimage'), { recursive: true, force: true });

  // The packages install system-wide, to /usr, as package `uwupulse`. Tauri
  // names the package after productName in kebab case, which would make
  // "UwUPulse" `uw-u-pulse`; the menu entry keeps saying UwUPulse through the
  // template in tauri.conf.json. The app looks for `uwupulse` when it checks that
  // dpkg or rpm owns it (apps/desktop/src-tauri/src/updates.rs). `tauri
  // bundle` packs the binary just built: with `tauri build`, the other
  // productName rebuilt and re-linked the whole app.
  console.log(`\n▸ Packaging UwUPulse ${version} as .deb and .rpm`);
  run(
    `pnpm --filter @uwupulse/desktop tauri bundle --bundles deb,rpm${targetArg} --config "${configFile('packages', { productName: 'uwupulse' })}"`,
  );
  const deb = join(out, `UwUPulse-linux-${arch()}.deb`);
  copyFileSync(
    only(join(bundles, 'deb'), (name) => name.endsWith('.deb'), 'deb'),
    deb,
  );
  const rpm = join(out, `UwUPulse-linux-${arch()}.rpm`);
  copyFileSync(
    only(join(bundles, 'rpm'), (name) => name.endsWith('.rpm'), 'rpm'),
    rpm,
  );
  produced.push(deb, rpm);

  // One folder, UwUPulse/, that runs where it lands. tar, not zip: the AppDir
  // needs its modes and symlinks.
  console.log('\n▸ Packing the portable folder');
  const staging = mkdtempSync(join(tmpdir(), 'uwupulse-portable-'));
  const folder = join(staging, 'UwUPulse');
  execFileSync('cp', ['-a', join(apps, 'UwUPulse'), folder]);
  const launcher = join(folder, 'uwupulse');
  writeFileSync(
    launcher,
    '#!/bin/sh\n# Starts UwUPulse from this folder.\nhere=$(dirname "$(readlink -f "$0")")\nexec "$here/AppRun" "$@"\n',
  );
  chmodSync(launcher, 0o755);
  writeFileSync(
    join(folder, 'README.txt'),
    [
      `UwUPulse ${version}, portable`,
      '',
      'Start it with ./uwupulse (or ./AppRun) in this folder. Nothing is installed:',
      'the folder can live anywhere and be deleted when you are done.',
      '',
      'This copy does not update itself. For updates, install the .deb or .rpm',
      '(or the AUR package uwupulse-bin) instead, or fetch the newest',
      'UwUPulse-linux-<arch>-portable.tar.gz from',
      'https://github.com/MinifyX/UwUPulse/releases/latest',
      '',
    ].join('\n'),
  );
  const portable = join(out, `UwUPulse-linux-${arch()}-portable.tar.gz`);
  execFileSync('tar', [
    '--owner=0',
    '--group=0',
    '--numeric-owner',
    '-czf',
    portable,
    '-C',
    staging,
    'UwUPulse',
  ]);
  rmSync(staging, { recursive: true, force: true });
  produced.push(portable);

  // Copies the setup installed (~/.local/share/uwupulse) update by running the
  // next setup. Like the rest of the suite, only for x64.
  if (arch() === 'x64') {
    const obs = await obsPlugin();
    if (obs) publishObsLinux(obs);
    console.log('\n▸ Packing it into the setup, for the updater');
    const setupConfig = configFile('setup', { bundle: { active: true, targets: ['appimage'] } });
    run(
      `pnpm --filter @uwupulse/setup tauri build --bundles appimage${targetArg} --config "${setupConfig}"`,
      { UWUPULSE_SETUP_PAYLOAD: apps, UWUPULSE_SETUP_OBS_PAYLOAD: obs ?? '' },
    );
    const image = join(out, `UwUPulse-update-linux-${arch()}.AppImage`);
    copyFileSync(
      only(
        join(bundles, 'appimage'),
        (name) => name.endsWith('.AppImage'),
        'AppImage of the setup',
      ),
      image,
    );
    execFileSync('chmod', ['+x', image]);
    produced.push(image);
  }
  rmSync(apps, { recursive: true, force: true });
}

console.log('\n▸ Nothing is signed for the updater here; `pnpm release` does that.');
for (const file of produced) console.log(`✧ ${file}`);
