// Publishes the UwUPulse version in tauri.conf.json, for Windows, macOS and
// Linux, from the machine that holds the update signing key.
//
//   pnpm release                  fetch what CI built for the tag (Windows x64
//                                 and ARM, macOS, Linux), sign what the updater
//                                 runs, check everything, publish it
//   pnpm release --build-windows  the same, but build the Windows x64 setup here
//                                 (on Windows) instead of taking CI's
//   pnpm release --no-build       take the Windows x64 setup already in
//                                 target/installers instead of CI's
//   pnpm release --windows-only   only Windows x64, built here, when CI can't
//                                 help (with --no-build: the one already built)
//   pnpm release --dry-run        everything up to publishing: fetch, sign and
//                                 check, then stop. Works on any pushed commit
//                                 with a green Installers run (a `ci/…` branch),
//                                 no tag needed; nothing leaves this machine.
//
// Needs a clean tree whose HEAD carries the pushed tag v<version>, a
// `## <version>` section in CHANGELOG.md (the release notes; a
// release-notes/<version>.json with `de` and `en` replaces them in the app if
// present), the GitHub CLI signed in with write access and the update signing
// key: TAURI_SIGNING_PRIVATE_KEY + TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or a
// folder with uwupulse-update.key and PASSWORT.txt in UWUPULSE_UPDATE_KEY_DIR
// (default: /Volumes/backups/DevKeys/UwUPulse-Update-Schluessel).
//
// The key never leaves this machine: CI (.github/workflows/installers.yml)
// builds everything unsigned on every push to main, and this script
// downloads it and signs the files the updater runs here. Only a Windows x64
// setup built here (--build-windows, --windows-only) needs Windows.
//
// The release's files carry no version in their names (UwUPulse-windows-x64-setup.exe,
// UwUPulse-linux-arm64.deb, …), so a link to the newest one never changes.
// Installed apps, though, accept an update only when its signature names the
// versioned file they expect (`UwUPulse-Setup-<version>.exe`, …): that binds the
// signed bytes to the version the feed claims. So each file is signed as a
// copy under that versioned name, the same bytes are published under the
// plain name, and the feed points there — old and new apps both accept it.
//
// Creates the GitHub release with every file and a SHA256SUMS.txt, updates the
// feeds on the `updates` branch (creating it the first time) and writes the AUR
// package for it (see the end of this file).

import { execFileSync } from 'node:child_process';
import { createHash, createPublicKey, verify } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { aurFiles, writeAur } from './aur.mjs';
import { FEED_BRANCH, REPOSITORY, releaseFeeds } from './release-feeds.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const git = (args, cwd = root) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
const fail = (message) => {
  console.error(`\n✗ ${message}`);
  process.exit(1);
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let token; // GitHub token for the API, looked up once
const noBuild = process.argv.includes('--no-build');
const windowsOnly = process.argv.includes('--windows-only');
const dryRun = process.argv.includes('--dry-run');
// The Windows x64 setup from this machine rather than from CI.
const localWindows = windowsOnly || noBuild || process.argv.includes('--build-windows');
const build = localWindows && !noBuild;
// What a real release insists on and a dry run only reports: the tag, the
// release notes, a version not released yet.
const insist = (ok, message) => {
  if (ok) return;
  if (!dryRun) fail(message);
  console.log(`  (dry run, ignored) ${message}`);
};
const conf = JSON.parse(readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
const version = conf.version;
const tag = `v${version}`;
const installers = join(root, 'target', 'installers');
const windowsName = 'UwUPulse-windows-x64-setup.exe';
const windowsSetup = join(installers, windowsName);

/**
 * What the release carries: each file under its published name, the CI
 * artifact it comes from (none: from this machine), and for the ones an installed app
 * updates itself with, `sign`: the feed's platform key → the versioned name the
 * signature has to carry. Those names are what installed apps check
 * (`setup_name` in apps/desktop/src-tauri/src/updates.rs); never change one,
 * or every install that expects it stops updating.
 */
const linux = (arch, rust) => [
  {
    artifact: `installers-linux-${arch}`,
    file: `UwUPulse-linux-${arch}.deb`,
    sign: { [`linux-${rust}-deb`]: `UwUPulse-${version}-linux-${rust}.deb` },
  },
  {
    artifact: `installers-linux-${arch}`,
    file: `UwUPulse-linux-${arch}.rpm`,
    sign: { [`linux-${rust}-rpm`]: `UwUPulse-${version}-linux-${rust}.rpm` },
  },
  { artifact: `installers-linux-${arch}`, file: `UwUPulse-linux-${arch}-portable.tar.gz` },
];
const PLATFORMS = [
  {
    artifact: localWindows ? null : 'installers-windows-x64',
    file: windowsName,
    sign: { 'windows-x86_64': `UwUPulse-Setup-${version}.exe` },
  },
  {
    artifact: 'installers-windows-arm64',
    file: 'UwUPulse-windows-arm64-setup.exe',
    sign: { 'windows-aarch64': `UwUPulse-Setup-${version}-windows-arm64.exe` },
  },
  { artifact: 'installers-macos-universal', file: 'UwUPulse-macos-universal.dmg' },
  {
    // One universal program for both Mac platforms, signed once under each name.
    artifact: 'installers-macos-universal',
    file: 'UwUPulse-update-macos-universal',
    sign: {
      'darwin-aarch64': `UwUPulse-Setup-${version}-macos-arm64-update`,
      'darwin-x86_64': `UwUPulse-Setup-${version}-macos-x64-update`,
    },
  },
  ...linux('x64', 'x86_64'),
  ...linux('arm64', 'aarch64'),
  {
    // For copies an earlier setup AppImage installed into ~/.local/share/uwupulse.
    artifact: 'installers-linux-x64',
    file: 'UwUPulse-update-linux-x64.AppImage',
    sign: { 'linux-x86_64': `UwUPulse-Setup-${version}-linux-x64.AppImage` },
  },
  // The OBS plugin on its own, unsigned: the setups above carry it too, inside
  // what the updater signature covers. With --build-windows the x64 zip still
  // comes from CI.
  { artifact: 'installers-windows-x64', file: 'UwUPulse-obs-windows-x64.zip' },
  { artifact: 'installers-windows-arm64', file: 'UwUPulse-obs-windows-arm64.zip' },
  { artifact: 'installers-macos-universal', file: 'UwUPulse-obs-macos-universal.zip' },
  { artifact: 'installers-linux-x64', file: 'UwUPulse-obs-linux-x64.tar.gz' },
  { artifact: 'installers-linux-x64', file: 'UwUPulse-obs-linux-x64.deb' },
].filter((entry) => !windowsOnly || entry.file === windowsName);

console.log(`\n▸ Checking UwUPulse ${version}`);
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version)) fail(`Unexpected version ${version}`);
insist(
  !git(['status', '--porcelain', '--untracked-files=no']),
  'The working tree has uncommitted changes.',
);
const head = git(['rev-parse', 'HEAD']);
let tagged = '';
try {
  tagged = git(['rev-parse', `${tag}^{commit}`]);
} catch {
  // Reported just below.
}
if (!tagged) {
  insist(false, `Tag ${tag} is missing. Tag the release commit and push the tag first.`);
} else {
  insist(tagged === head, `HEAD isn't ${tag}. Check out the tag first.`);
  const pushed = git(['ls-remote', '--tags', 'origin', `refs/tags/${tag}`]).split(/\s/)[0];
  insist(pushed === git(['rev-parse', `refs/tags/${tag}`]), `Push ${tag} first.`);
}
if (!ghToken()) fail('Sign in to the GitHub CLI first (gh auth login).');
insist(!(await github(`releases/tags/${tag}`)), `${tag} is already released.`);
const changes = changelog(version);
insist(changes, `CHANGELOG.md has no "## ${version}" section.`);
let notes = {
  de: changes || '(Versionshinweise fehlen)',
  en: changes || '(release notes missing)',
};
const notesFile = join(root, 'release-notes', `${version}.json`);
if (existsSync(notesFile)) {
  notes = JSON.parse(readFileSync(notesFile, 'utf8'));
  if (typeof notes.de !== 'string' || typeof notes.en !== 'string') {
    fail('The release notes need de and en.');
  }
}
const key = signingKey();

if (build) {
  if (process.platform !== 'win32') fail('Building the Windows setup needs Windows.');
  console.log('\n▸ Building the Windows setup');
  // Without the key: the build runs third-party build scripts.
  const env = { ...process.env };
  delete env.TAURI_SIGNING_PRIVATE_KEY;
  delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  execFileSync(process.execPath, [join(root, 'scripts', 'build-setup.mjs')], {
    cwd: root,
    stdio: 'inherit',
    env,
  });
} else if (localWindows && !existsSync(windowsSetup)) {
  fail(`${windowsSetup} is missing. Run without --no-build.`);
} else if (
  localWindows &&
  statSync(windowsSetup).mtimeMs < Number(git(['log', '-1', '--format=%ct'])) * 1000
) {
  fail('The setup in target/installers is older than the release commit. Run without --no-build.');
}

const work = mkdtempSync(join(tmpdir(), 'uwupulse-release-'));
try {
  const files = new Map(localWindows ? [[windowsName, windowsSetup]] : []);
  if (!windowsOnly) {
    console.log(
      `\n▸ Fetching what CI built for this ${dryRun ? 'commit' : 'tag'}: ${localWindows ? '' : 'Windows x64, '}Windows ARM, macOS, Linux`,
    );
    const ci = join(work, 'ci');
    const run = await ciRun(head);
    execFileSync('gh', ['run', 'download', String(run), '--repo', REPOSITORY, '--dir', ci], {
      stdio: 'inherit',
    });
    for (const entry of PLATFORMS.filter((p) => p.artifact)) {
      const from = join(ci, entry.artifact, entry.file);
      if (!existsSync(from)) fail(`CI left no ${entry.file} in ${entry.artifact}.`);
      const to = join(installers, entry.file);
      mkdirSync(installers, { recursive: true });
      copyFileSync(from, to);
      files.set(entry.file, to);
    }
  }

  // Each under the versioned name its installed apps check for: a copy of the
  // same bytes, in a folder of its own, so `tauri signer` writes that name into
  // the signature's trusted comment.
  console.log('\n▸ Signing what the updater runs, and checking it against the updater key');
  const signed = {};
  const signing = join(work, 'sign');
  mkdirSync(signing);
  for (const entry of PLATFORMS.filter((p) => p.sign)) {
    const bytes = readFileSync(files.get(entry.file));
    for (const [platform, name] of Object.entries(entry.sign)) {
      const copy = join(signing, name);
      writeFileSync(copy, bytes);
      execFileSync(
        'pnpm',
        ['--filter', '@uwupulse/desktop', 'exec', 'tauri', 'signer', 'sign', copy],
        {
          cwd: root,
          stdio: 'inherit',
          shell: process.platform === 'win32',
          env: { ...process.env, ...key },
        },
      );
      if (!existsSync(`${copy}.sig`)) fail(`${name}.sig is missing: it wasn't signed.`);
      const signature = readFileSync(`${copy}.sig`, 'utf8').trim();
      checkSignature(bytes, signature, conf.plugins.updater.pubkey, name);
      signed[platform] = { name: entry.file, signature };
      console.log(`  ✓ ${platform}: ${entry.file}, signed as ${name}`);
    }
  }

  // A checksum for every file of the release, LF-only so `sha256sum -c` reads it.
  const sums = join(work, 'SHA256SUMS.txt');
  const lines = [...files.entries()].map(
    ([name, path]) => `${createHash('sha256').update(readFileSync(path)).digest('hex')}  ${name}`,
  );
  writeFileSync(sums, `${lines.join('\n')}\n`);
  files.set('SHA256SUMS.txt', sums);

  if (dryRun) {
    dryRunReport(files, signed, readFileSync(sums, 'utf8'));
    rmSync(work, { recursive: true, force: true });
    process.exit(0);
  }

  console.log(`\n▸ Creating the release on ${REPOSITORY}`);
  const notesPath = join(work, 'notes.md');
  writeFileSync(notesPath, releaseBody(await aurExists('uwupulse-bin')));
  const channel = version.includes('-') ? '--prerelease' : '--latest';
  execFileSync(
    'gh',
    [
      'release',
      'create',
      tag,
      ...files.values(),
      '--repo',
      REPOSITORY,
      '--verify-tag',
      '--title',
      `UwUPulse ${version}`,
      '--notes-file',
      notesPath,
      channel,
    ],
    { stdio: 'inherit' },
  );

  const feeds = releaseFeeds({ version, notes, setups: signed });
  console.log(`\n▸ Updating ${Object.keys(feeds).join(', ')} on ${FEED_BRANCH}`);
  const dir = join(work, 'feeds');
  const remote = `https://github.com/${REPOSITORY}.git`;
  const exists = git(['ls-remote', '--heads', remote, FEED_BRANCH], work) !== '';
  if (exists) {
    git(['clone', '-q', '--depth', '1', '--branch', FEED_BRANCH, remote, dir], work);
  } else {
    // First release: the branch holds nothing but the feeds.
    git(['init', '-q', '-b', FEED_BRANCH, dir], work);
    git(['remote', 'add', 'origin', remote], dir);
    writeFileSync(
      join(dir, 'README.md'),
      '# Update feeds\n\nWritten by `pnpm release` on the main branch. Installed UwUPulse apps read ' +
        '`stable.json` or `beta.json` from here, depending on their update channel.\n',
    );
  }
  for (const [name, feed] of Object.entries(feeds)) {
    writeFileSync(join(dir, name), `${JSON.stringify(feed, null, 2)}\n`);
  }
  git(['add', '--all'], dir);
  git(
    [
      '-c',
      'user.name=UwUPulse Release',
      '-c',
      'user.email=release@uwupulse.invalid',
      'commit',
      '-qm',
      `UwUPulse ${version}`,
    ],
    dir,
  );
  execFileSync('git', ['push', '-q', 'origin', `HEAD:${FEED_BRANCH}`], {
    cwd: dir,
    stdio: 'inherit',
  });

  console.log("\n▸ Checking what's online");
  const release = await github(`releases/tags/${tag}`);
  for (const [name, path] of files) {
    const asset = release?.assets?.find((a) => a.name === name);
    if (!asset || asset.state !== 'uploaded') fail(`The release has no ${name}.`);
    if (asset.size !== statSync(path).size) fail(`The published ${name} has a different size.`);
  }
  const feedName = version.includes('-') ? 'beta.json' : 'stable.json';
  const file = await github(`contents/${feedName}?ref=${FEED_BRANCH}`);
  const update = file && JSON.parse(Buffer.from(file.content, 'base64').toString('utf8'));
  if (update?.version !== version) fail(`${feedName} wasn't updated.`);
  for (const [platform, setup] of Object.entries(signed)) {
    const entry = update.platforms?.[platform];
    const asset = release.assets.find((a) => a.name === setup.name);
    if (entry?.signature !== setup.signature) fail(`${feedName} has no ${platform}.`);
    if (entry.url !== asset.browser_download_url)
      fail(`${feedName} points elsewhere for ${platform}.`);
  }
  console.log(`\n✧ UwUPulse ${version} is out: ${release.html_url}`);
  console.log(
    `  ${feedName} updated for ${Object.keys(signed).join(', ')}${version.includes('-') ? ' (Beta channel)' : ''}`,
  );

  if (!windowsOnly) aur(readFileSync(sums, 'utf8'));
} finally {
  rmSync(work, { recursive: true, force: true });
}

/**
 * What a release would publish, without publishing anything: the files, the
 * feed entries and the AUR package, each built the way the release builds it.
 */
function dryRunReport(files, signed, sums) {
  console.log('\n▸ Dry run: the release would carry');
  for (const [name, path] of files) {
    console.log(`  ${name} (${(statSync(path).size / 1024 / 1024).toFixed(1)} MiB)`);
  }
  const feeds = releaseFeeds({ version, notes, setups: signed });
  for (const [name, feed] of Object.entries(feeds)) {
    console.log(`  ${FEED_BRANCH}/${name}: ${Object.keys(feed.platforms).join(', ')}`);
  }
  releaseBody(false);
  const packaged = aurFiles({ version, sums });
  console.log(`  AUR uwupulse-bin: ${Object.keys(packaged).join(', ')}`);
  console.log(`\n✧ Dry run of UwUPulse ${version} passed. Nothing was published.`);
}

/** The release page: what changed, then which file is for which system. */
function releaseBody(aurLive) {
  const guide = `https://github.com/${REPOSITORY}#getting-started`;
  const has = (name) => PLATFORMS.some((p) => p.file === name);
  const code = (name) => `\`${name}\``;
  // [system (de), system (en), files] for each row whose files this release has.
  const rows = [
    ['Windows (x64)', 'Windows (x64)', code(windowsName)],
    ['Windows auf ARM', 'Windows on ARM', code('UwUPulse-windows-arm64-setup.exe')],
    [
      'macOS (Intel & Apple-Chip)',
      'macOS (Intel & Apple chip)',
      code('UwUPulse-macos-universal.dmg'),
    ],
    [
      'Ubuntu / Debian',
      'Ubuntu / Debian',
      `${code('UwUPulse-linux-x64.deb')} · ARM: ${code('UwUPulse-linux-arm64.deb')}`,
    ],
    [
      'Fedora / openSUSE',
      'Fedora / openSUSE',
      `${code('UwUPulse-linux-x64.rpm')} · ARM: ${code('UwUPulse-linux-arm64.rpm')}`,
    ],
    // The AUR package only once it exists there; AUR registration can be closed for a while.
    aurLive
      ? ['Arch Linux', 'Arch Linux', 'AUR: `yay -S uwupulse-bin`']
      : [
          'Arch Linux',
          'Arch Linux',
          `${code('UwUPulse-linux-x64-portable.tar.gz')} (AUR: ${code('uwupulse-bin')} folgt / coming)`,
        ],
    [
      'Linux portabel',
      'Linux portable',
      `${code('UwUPulse-linux-x64-portable.tar.gz')} · ARM: ${code('…-arm64-portable.tar.gz')}`,
    ],
    [
      'OBS-Plugin (OBS 31+), ohne Setup',
      'OBS plugin (OBS 31+), without the setup',
      [
        'UwUPulse-obs-windows-x64.zip',
        'UwUPulse-obs-windows-arm64.zip',
        'UwUPulse-obs-macos-universal.zip',
        'UwUPulse-obs-linux-x64.tar.gz',
        'UwUPulse-obs-linux-x64.deb',
      ]
        .map(code)
        .join(' · '),
    ],
  ].filter(([, , files]) => {
    const named = [...files.matchAll(/`(UwUPulse-[^`]+)`/g)].map((m) => m[1]);
    return named.length === 0 ? !windowsOnly : named.every(has);
  });
  const table = (column) =>
    ['| | |', '|---|---|', ...rows.map((row) => `| ${row[column]} | ${row[2]} |`)].join('\n');
  const mac = has('UwUPulse-macos-universal.dmg');
  const de = [
    table(0),
    'Windows: warnt es („Der Computer wurde durch Windows geschützt“), **Weitere Informationen → Trotzdem ausführen**.',
    ...(mac
      ? [
          'macOS: die `.dmg` öffnen und **UwUPulse Setup** starten. UwUPulse ist nicht bei Apple notarisiert (keine Developer ID): sagt macOS, es könne das Programm nicht prüfen, unter **Systemeinstellungen → Datenschutz & Sicherheit → Trotzdem öffnen** freigeben.',
        ]
      : []),
    ...(windowsOnly
      ? []
      : [
          'Linux: `.deb` und `.rpm` installieren systemweit und aktualisieren sich selbst (fragt nach dem Administrator-Passwort); die portable Version einfach entpacken und `./UwUPulse/uwupulse` starten, sie aktualisiert sich nicht.',
          'OBS-Plugin: Die Setups für Windows, macOS und Linux (x64) bieten es gleich mit an und halten es aktuell. Von Hand: den Ordner aus der `.zip` bzw. `.tar.gz` in den Plugin-Ordner von OBS kopieren (siehe `plugins/obs/README.md`), oder unter Ubuntu/Debian `UwUPulse-obs-linux-x64.deb` installieren.',
        ]),
  ];
  const en = [
    table(1),
    'Windows: if it warns that it "protected your PC", **More info → Run anyway**.',
    ...(mac
      ? [
          "macOS: open the `.dmg` and start **UwUPulse Setup**. UwUPulse isn't notarized by Apple (no developer ID): if macOS says it can't check the app, allow it under **System Settings → Privacy & Security → Open Anyway**.",
        ]
      : []),
    ...(windowsOnly
      ? []
      : [
          "Linux: the `.deb` and `.rpm` install system-wide and update themselves (asking for the administrator password); the portable one you just unpack and start with `./UwUPulse/uwupulse`, and it doesn't update itself.",
          "OBS plugin: the setups for Windows, macOS and Linux (x64) offer it as well and keep it up to date. By hand: copy the folder from the `.zip` or `.tar.gz` into OBS's plugin folder (see `plugins/obs/README.md`), or install `UwUPulse-obs-linux-x64.deb` on Ubuntu/Debian.",
        ]),
  ];
  return [
    ...(notes.de === notes.en
      ? [`## Änderungen · Changes\n\n${notes.en}\n`]
      : [`## Deutsch\n\n${notes.de}\n`, `## English\n\n${notes.en}\n`]),
    `## Herunterladen\n\n${de.join('\n\n')}\n\n[Anleitung](${guide})\n`,
    `## Downloads\n\n${en.join('\n\n')}\n\n[Install guide](${guide})\n`,
    'Prüfsummen · checksums: `SHA256SUMS.txt`. Die `UwUPulse-update-…`-Dateien sind für den Updater in der App · the `UwUPulse-update-…` files are for the in-app updater.\n',
  ].join('\n');
}

/**
 * The AUR package uwupulse-bin for this release. With UWUPULSE_AUR_DIR pointing at
 * a checkout of ssh://aur@aur.archlinux.org/uwupulse-bin.git it is committed and
 * pushed from there; otherwise it lands in target/aur/uwupulse-bin, and CI's
 * `aur` job (.github/workflows/aur.yml) pushes it once AUR_SSH_PRIVATE_KEY is set.
 */
function aur(sums) {
  const files = aurFiles({ version, sums });
  const checkout = process.env.UWUPULSE_AUR_DIR;
  if (!checkout) {
    const dir = join(root, 'target', 'aur', 'uwupulse-bin');
    writeAur(dir, files);
    console.log(`\n▸ AUR: PKGBUILD and .SRCINFO in ${dir}`);
    console.log(
      '  CI pushes them (aur.yml) when AUR_SSH_PRIVATE_KEY is set. By hand: copy both into a\n' +
        '  checkout of ssh://aur@aur.archlinux.org/uwupulse-bin.git, commit and push — or set\n' +
        '  UWUPULSE_AUR_DIR to that checkout next time.',
    );
    return;
  }
  if (!existsSync(join(checkout, '.git')))
    fail(`UWUPULSE_AUR_DIR (${checkout}) is no git checkout.`);
  console.log(`\n▸ AUR: uwupulse-bin ${version} from ${checkout}`);
  git(['pull', '-q', '--ff-only'], checkout);
  writeAur(checkout, files);
  git(['add', 'PKGBUILD', '.SRCINFO'], checkout);
  if (!git(['status', '--porcelain'], checkout)) {
    console.log('  already up to date');
    return;
  }
  git(['commit', '-qm', `UwUPulse ${version}`], checkout);
  execFileSync('git', ['push', '-q'], { cwd: checkout, stdio: 'inherit' });
  console.log('  ✓ pushed');
}

/**
 * The Installers run for this commit, once it has finished green. Only runs
 * from main of this repository count (a dry run also takes `ci/**`), started
 * by a push or by hand: what gets signed must come from a branch only
 * maintainers write to.
 */
async function ciRun(sha) {
  const started = Date.now();
  let announced = false;
  const trusted = (run) =>
    ['push', 'workflow_dispatch'].includes(run.event) &&
    (run.headBranch === 'main' || (dryRun && run.headBranch.startsWith('ci/')));
  for (;;) {
    const runs = JSON.parse(
      execFileSync(
        'gh',
        [
          'run',
          'list',
          '--repo',
          REPOSITORY,
          '--workflow',
          'installers.yml',
          '--commit',
          sha,
          ...(dryRun ? [] : ['--branch', 'main']),
          '--json',
          'databaseId,status,conclusion,event,headBranch',
          '--limit',
          '20',
        ],
        { encoding: 'utf8' },
      ),
    ).filter(trusted);
    const done = runs.find((run) => run.status === 'completed' && run.conclusion === 'success');
    if (done) {
      const from = execFileSync(
        'gh',
        [
          'api',
          `repos/${REPOSITORY}/actions/runs/${done.databaseId}`,
          '--jq',
          '.head_repository.full_name',
        ],
        { encoding: 'utf8' },
      ).trim();
      if (from !== REPOSITORY) fail(`Installers run ${done.databaseId} was built from ${from}.`);
      return done.databaseId;
    }
    const running = runs.find((run) => run.status !== 'completed');
    if (!running && runs.length > 0 && Date.now() - started > 60_000) {
      fail(
        `The Installers run for ${sha.slice(0, 7)} failed. Fix it, or release Windows x64 alone with --windows-only (on Windows).`,
      );
    }
    if (Date.now() - started > 90 * 60_000) fail('Gave up waiting for CI after 90 minutes.');
    if (!announced) {
      console.log('  waiting for CI to finish the builds…');
      announced = true;
    }
    await sleep(30_000);
  }
}

/** The `## <version>` section of CHANGELOG.md, without its heading; '' if there is none. */
function changelog(wanted) {
  const text = readFileSync(join(root, 'CHANGELOG.md'), 'utf8');
  const lines = text.split('\n');
  const start = lines.findIndex((line) => line.trim() === `## ${wanted}`);
  if (start < 0) return '';
  const end = lines.findIndex((line, i) => i > start && line.startsWith('## '));
  return lines
    .slice(start + 1, end < 0 ? undefined : end)
    .join('\n')
    .trim();
}

/** The update signing key from the environment or the key folder. */
function signingKey() {
  if (process.env.TAURI_SIGNING_PRIVATE_KEY) {
    return {
      TAURI_SIGNING_PRIVATE_KEY: process.env.TAURI_SIGNING_PRIVATE_KEY,
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? '',
    };
  }
  const folder =
    process.env.UWUPULSE_UPDATE_KEY_DIR || '/Volumes/backups/DevKeys/UwUPulse-Update-Schluessel';
  const file = join(folder, 'uwupulse-update.key');
  if (!existsSync(file)) {
    fail('No signing key: set TAURI_SIGNING_PRIVATE_KEY(_PASSWORD) or UWUPULSE_UPDATE_KEY_DIR.');
  }
  const password = join(folder, 'PASSWORT.txt');
  return {
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(file, 'utf8').trim(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: existsSync(password)
      ? readFileSync(password, 'utf8').trim()
      : '',
  };
}

/** Verifies a Tauri updater signature (base64 minisign) the way installed apps do. */
function checkSignature(file, signatureBase64, pubkeyBase64, expectedName) {
  const lines = (text) => Buffer.from(text, 'base64').toString('utf8').split(/\r?\n/);
  const pub = Buffer.from(lines(pubkeyBase64)[1], 'base64');
  const [, signatureLine, trustedLine, globalLine] = lines(signatureBase64);
  const sig = Buffer.from(signatureLine, 'base64');
  if (pub.length !== 42 || sig.length !== 74) fail('Malformed key or signature.');
  if (!sig.subarray(2, 10).equals(pub.subarray(2, 10))) {
    fail(`${expectedName} was signed with a different key.`);
  }
  const publicKey = createPublicKey({
    key: { kty: 'OKP', crv: 'Ed25519', x: pub.subarray(10).toString('base64url') },
    format: 'jwk',
  });
  const algorithm = sig.subarray(0, 2).toString('latin1');
  const signed = algorithm === 'ED' ? createHash('blake2b512').update(file).digest() : file;
  if (!verify(null, signed, publicKey, sig.subarray(10))) {
    fail(`The signature doesn't match ${expectedName}.`);
  }
  const trusted = Buffer.from(trustedLine.replace(/^trusted comment: /, ''), 'utf8');
  if (
    !verify(
      null,
      Buffer.concat([sig.subarray(10), trusted]),
      publicKey,
      Buffer.from(globalLine, 'base64'),
    )
  ) {
    fail(`The trusted comment of ${expectedName}'s signature doesn't verify.`);
  }
  // Installed apps refuse a setup whose signature names another file: that is
  // what ties the feed's version to the signed setup.
  const names = trusted
    .toString('utf8')
    .split('\t')
    .map((part) => part.trim().replace(/^file:/, ''));
  if (!names.includes(expectedName)) fail(`The signature doesn't name ${expectedName}.`);
}

/** Whether the AUR has this package; false when it doesn't or doesn't answer. */
async function aurExists(name) {
  try {
    const response = await fetch(`https://aur.archlinux.org/rpc/v5/info?arg[]=${name}`, {
      headers: { 'user-agent': 'uwupulse-release' },
      signal: AbortSignal.timeout(15_000),
    });
    return response.ok && (await response.json()).resultcount > 0;
  } catch {
    return false;
  }
}

async function github(path) {
  const headers = { accept: 'application/vnd.github+json', 'user-agent': 'uwupulse-release' };
  token ??= process.env.GH_TOKEN || process.env.GITHUB_TOKEN || ghToken();
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`https://api.github.com/repos/${REPOSITORY}/${path}`, { headers });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${path}`);
  return response.json();
}

function ghToken() {
  try {
    return execFileSync('gh', ['auth', 'token'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
  } catch {
    return '';
  }
}
