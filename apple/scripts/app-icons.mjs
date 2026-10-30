#!/usr/bin/env node
// Renders the iPhone and Apple Watch app icons from brand/uwupulse-app-icon.svg.
//
//   node apple/scripts/app-icons.mjs        (after `pnpm install`, on macOS)
//
// iOS: the tile as a full-bleed square, iOS draws its own corners.
// watchOS: the same square with the artwork pulled in a little, so the stars
// and the pulse line stay inside the round mask.
// Both are flattened (no alpha channel), as App Store Connect requires.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const svg = readFileSync(join(root, "brand/uwupulse-app-icon.svg"), "utf8");
const tile = '<rect width="512" height="512" rx="116" fill="url(#uwu-bg)"/>';
if (!svg.includes(tile)) throw new Error("tile rect not found in uwupulse-app-icon.svg");

const square = svg.replace(tile, '<rect width="512" height="512" fill="url(#uwu-bg)"/>');
const [head, rest] = square.split('<rect width="512" height="512" fill="url(#uwu-bg)"/>');
const body = rest.replace(/<\/svg>\s*$/, "");
const watch =
  `${head}<rect width="512" height="512" fill="url(#uwu-bg)"/>` +
  `<g transform="translate(256 262) scale(0.84) translate(-256 -256)">${body}</g></svg>\n`;

const work = mkdtempSync(join(tmpdir(), "uwupulse-icons-"));
const targets = [
  [square, "apple/UwUPulse/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon.png"],
  [watch, "apple/UwUPulseWatch/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon.png"],
];
try {
  targets.forEach(([source, out], i) => {
    const file = join(work, `icon-${i}.svg`);
    const dir = join(work, `out-${i}`);
    writeFileSync(file, source);
    execFileSync("pnpm", ["--dir", join(root, "apps/desktop"), "exec", "tauri", "icon", file, "-o", dir, "-p", "1024"], {
      stdio: "ignore",
    });
    // PNG → JPEG (quality 100) → PNG drops the alpha channel.
    const jpg = join(work, `icon-${i}.jpg`);
    execFileSync("sips", ["-s", "format", "jpeg", "-s", "formatOptions", "100", join(dir, "1024x1024.png"), "--out", jpg], { stdio: "ignore" });
    execFileSync("sips", ["-s", "format", "png", jpg, "--out", join(work, `flat-${i}.png`)], { stdio: "ignore" });
    renameSync(join(work, `flat-${i}.png`), join(root, out));
    console.log(`✓ ${out}`);
  });
} finally {
  rmSync(work, { recursive: true, force: true });
}
