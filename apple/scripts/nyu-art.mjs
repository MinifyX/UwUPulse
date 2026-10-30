#!/usr/bin/env node
// Writes Nyu's mood stickers (heart shell, from brand/uwupulse-symbol.svg) into the iOS asset
// catalogue as vector imagesets: Nyu-happy, Nyu-sleepy, Nyu-sad (empty states, about screen).
//   node apple/scripts/nyu-art.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const assets = resolve(import.meta.dirname, "../UwUPulse/Resources/Assets.xcassets");
const INK = "#4B1D3F";
const HEART =
  "M256 424 C176 374 88 310 88 220 C88 160 134 120 188 120 C222 120 244 136 256 158 C268 136 290 120 324 120 C378 120 424 160 424 220 C424 310 336 374 256 424 Z";
const EARS = ["M128 196 L130 92 Q132 72 150 82 L232 138 Z", "M384 196 L382 92 Q380 72 362 82 L280 138 Z"];
const line = `fill="none" stroke="${INK}" stroke-width="13"`;
const W = "M226 294 q15 22 30 0 q15 22 30 0";

const faces = {
  happy:
    `<g fill="${INK}"><ellipse cx="202" cy="248" rx="17" ry="22"/><ellipse cx="310" cy="248" rx="17" ry="22"/></g>` +
    `<g fill="#FFFFFF"><circle cx="208" cy="239" r="6.5"/><circle cx="316" cy="239" r="6.5"/></g>` +
    `<path d="${W}" ${line}/>`,
  sleepy:
    `<path d="M178 250 q24 16 48 0" ${line}/><path d="M286 250 q24 16 48 0" ${line}/>` +
    `<path d="M240 298 q8 11 16 0 q8 11 16 0" ${line}/>`,
  sad:
    `<path d="M178 258 q24 -22 48 0" ${line}/><path d="M286 258 q24 -22 48 0" ${line}/>` +
    `<g fill="#9ED8FF" stroke="${INK}" stroke-width="6"><path d="M186 274 q-12 18 0 27 q12 -9 0 -27Z"/><path d="M326 274 q-12 18 0 27 q12 -9 0 -27Z"/></g>` +
    `<path d="M234 310 Q256 292 278 310" ${line}/>`,
};

// Sleepy gets two lilac z's, with their own die-cut edge.
const zs = `<g stroke-linejoin="round" stroke-linecap="round" fill="none">
<path d="M392 70 h26 l-26 30 h26 M432 30 h18 l-18 20 h18" stroke="#FFFFFF" stroke-width="26"/>
<path d="M392 70 h26 l-26 30 h26 M432 30 h18 l-18 20 h18" stroke="${INK}" stroke-width="16"/>
<path d="M392 70 h26 l-26 30 h26 M432 30 h18 l-18 20 h18" stroke="#CDB8FF" stroke-width="7"/></g>`;

function sticker(mood) {
  const edge = [...EARS, HEART].map((d) => `<path d="${d}"/>`).join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="56 14 412 432" width="412" height="432">` +
    `<g stroke-linecap="round" stroke-linejoin="round">` +
    `<g fill="#FFFFFF" stroke="#FFFFFF" stroke-width="44">${edge}</g>` +
    `<g fill="#FF6FA6" stroke="${INK}" stroke-width="14">${EARS.map((d) => `<path d="${d}"/>`).join("")}</g>` +
    `<g fill="#FFB8D3"><path d="M150 140 L151 110 Q152 102 159 106 L196 132 Z"/><path d="M362 140 L361 110 Q360 102 353 106 L316 132 Z"/></g>` +
    `<path d="${HEART}" fill="#FF6FA6" stroke="${INK}" stroke-width="14"/>` +
    `<path d="M124 214 C124 182 146 160 176 156" fill="none" stroke="#FFB8D3" stroke-width="14"/>` +
    `<g fill="#FF4D8D" opacity="0.5"><ellipse cx="160" cy="286" rx="22" ry="13"/><ellipse cx="352" cy="286" rx="22" ry="13"/></g>` +
    faces[mood] +
    `</g>${mood === "sleepy" ? zs : ""}</svg>\n`
  );
}

for (const mood of Object.keys(faces)) {
  const dir = join(assets, `Nyu-${mood}.imageset`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `Nyu-${mood}.svg`), sticker(mood));
  writeFileSync(
    join(dir, "Contents.json"),
    JSON.stringify(
      {
        images: [{ filename: `Nyu-${mood}.svg`, idiom: "universal" }],
        info: { author: "xcode", version: 1 },
        properties: { "preserves-vector-representation": true },
      },
      null,
      2,
    ) + "\n",
  );
  console.log(`✓ Nyu-${mood}`);
}
