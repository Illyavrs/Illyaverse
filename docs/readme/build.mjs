// Renders the README images: hero, link buttons and a screenshot showcase,
// using the website's glass material over a background scene.
// Run from the repository root:  node docs/readme/build.mjs
import { chromium } from "playwright";
import sharp from "sharp";
import { mkdir, writeFile, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const assets = pathToFileURL(join(here, "assets")).href;
const W = 1200;

const arrow = `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>`;
const download = `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12M7 10l5 5 5-5M4 20h16"/></svg>`;
const book = `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4.5A2.5 2.5 0 0 1 6.5 2H20v17H6.5A2.5 2.5 0 0 0 4 21.5zM4 21.5A2.5 2.5 0 0 0 6.5 24H20"/></svg>`;

// Same material as the site: neutral gray-black glass, 14px radius.
const css = `
@font-face{font-family:Nunito;src:url("${assets}/nunito-latin-wght-normal.woff2");font-weight:200 1000}
*{box-sizing:border-box;margin:0}
html,body{width:${W}px;font-family:Nunito,"Microsoft JhengHei",sans-serif;color:#f5f5f5;-webkit-font-smoothing:antialiased}
.stage{position:relative;width:${W}px;overflow:hidden;background:#0a0a0c}
.bg{position:absolute;inset:0;background:url("${assets}/background-source.png") var(--pos,center) / cover no-repeat}
.tint{position:absolute;inset:0;background:rgb(8 8 10/var(--dim,.30))}
.glass{position:relative;background:rgb(24 24 27/.55);border:1px solid rgb(255 255 255/.14);border-radius:14px;
  backdrop-filter:blur(14px);-webkit-backdrop-filter:blur(14px);box-shadow:0 12px 36px rgb(0 0 0/.25)}
.pink{color:#f28eb6}
`;
const page = (body, h, vars = "") => `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head>
<body><div class="stage" style="height:${h}px;${vars}"><div class="bg"></div><div class="tint"></div>${body}</div></body></html>`;

// ---------------------------------------------------------------- hero
const links = ["Community maps", "Normal and RX rankings", "Community chat", "Profile and appearance"];
const hero = page(
  `<style>
    .card{position:absolute;left:40px;right:40px;top:40px;bottom:40px;overflow:hidden}
    .copy{position:absolute;left:52px;top:46px;width:640px}
    .word{font-size:112px;line-height:1;font-weight:900;letter-spacing:-.04em}
    .kind{margin-top:14px;font-size:20px;font-weight:800}
    .desc{margin-top:12px;font-size:18px;line-height:1.6;font-weight:500;color:#cfcfcf;max-width:560px}
    .stack{margin-top:18px;font-size:14px;font-weight:700;color:#a8a8a8;letter-spacing:.02em}
    .char{position:absolute;right:40px;top:-30px;height:860px}
    .strip{position:absolute;left:0;right:0;bottom:0;height:62px;display:grid;grid-template-columns:repeat(4,1fr);
           border-top:1px solid rgb(255 255 255/.12);background:rgb(20 20 23/.88)}
    .strip div{display:flex;align-items:center;justify-content:space-between;padding:0 24px;font-size:15px;font-weight:800;color:#e8e8e8}
    .strip div+div{border-left:1px solid rgb(255 255 255/.12)}
    .strip svg{color:#a8a8a8}
  </style>
  <div class="glass card">
    <div class="copy">
      <div class="word">Illya<span class="pink">verse</span></div>
      <div class="kind">osu! community server</div>
      <p class="desc">Accounts, beatmaps, scores, replays and chat on one self-hosted deployment, for both stable and lazer.</p>
      <div class="stack">Next.js · Rust · PostgreSQL · Docker</div>
    </div>
    <img class="char" src="${assets}/character.png">
    <div class="strip">${links.map((l) => `<div>${l}${arrow}</div>`).join("")}</div>
  </div>`,
  520,
  "--pos:center 30%;--dim:.22",
);

// ---------------------------------------------------------------- buttons
// One continuous strip, split into two halves so each half can be its own link.
const btn = (icon, title, sub, primary, left) =>
  `<div class="glass b" style="left:${left}px;${primary ? "background:#f28eb6;color:#111;border-color:#f6a9c7;backdrop-filter:none" : ""}">${icon}<div><div class="t">${title}</div><div class="s">${sub}</div></div></div>`;
const buttons = page(
  `<style>
    .b{position:absolute;top:14px;bottom:14px;width:340px;display:flex;align-items:center;gap:16px;padding:0 28px}
    .b svg{flex:none}
    .t{font-size:22px;font-weight:900;line-height:1.15}
    .s{font-size:14px;font-weight:700;opacity:.72;margin-top:2px}
  </style>
  ${btn(download, "Kaleidoscope launcher", "stable and lazer", true, 230)}
  ${btn(book, "Documentation", "Deploy, configure, connect", false, 630)}`,
  100,
  "--pos:center 22%;--dim:.22",
);

// ---------------------------------------------------------------- showcase
const showcase = page(
  `<style>
    .wrap{position:absolute;inset:40px}
    figure{position:absolute;margin:0}
    figure img{display:block;width:100%;border-radius:14px;border:1px solid rgb(255 255 255/.16);box-shadow:0 18px 50px rgb(0 0 0/.35)}
    figcaption{margin-top:12px;font-size:16px;font-weight:800;color:#f5f5f5;text-shadow:0 1px 8px rgb(0 0 0/.6)}
    figcaption span{font-weight:600;color:#d0d0d0}
  </style>
  <div class="wrap">
    <figure style="left:0;top:0;width:1120px"><img src="${assets}/shot-home.webp"><figcaption>Home <span>· latest community maps, recent scores, connection steps</span></figcaption></figure>
    <figure style="left:0;top:660px;width:550px"><img src="${assets}/shot-profile.webp"><figcaption>Profile <span>· rank history, Normal and RX</span></figcaption></figure>
    <figure style="left:570px;top:660px;width:550px"><img src="${assets}/shot-beatmaps.webp"><figcaption>Beatmaps <span>· osu! and server libraries</span></figcaption></figure>
  </div>`,
  1180,
  "--pos:center 70%;--dim:.45",
);

// ---------------------------------------------------------------- render
const jobs = [
  ["hero", hero, 520, W],
  ["buttons", buttons, 100, W],
  ["showcase", showcase, 1180, W],
];

const browser = await chromium.launch();
const ctx = await browser.newContext({ deviceScaleFactor: 2 });
const tmp = join(here, "out");
await mkdir(tmp, { recursive: true });
for (const [name, html, h, w] of jobs) {
  const file = join(tmp, `${name}.html`);
  await writeFile(file, html);
  const p = await ctx.newPage();
  await p.setViewportSize({ width: w, height: h });
  await p.goto(pathToFileURL(file).href);
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(300);
  const png = await p.screenshot({ clip: { x: 0, y: 0, width: w, height: h } });
  if (name === "buttons") {
    const half = W; // 2x device scale: each half is 600 css px wide
    await sharp(png).extract({ left: 0, top: 0, width: half, height: h * 2 }).webp({ quality: 90 }).toFile(join(here, "btn-kaleidoscope.webp"));
    await sharp(png).extract({ left: half, top: 0, width: half, height: h * 2 }).webp({ quality: 90 }).toFile(join(here, "btn-docs.webp"));
    await p.close();
    console.log("wrote btn-kaleidoscope.webp, btn-docs.webp");
    continue;
  }
  await sharp(png).webp({ quality: 90 }).toFile(join(here, `${name}.webp`));
  await p.close();
  console.log("wrote", `${name}.webp`);
}
await browser.close();
await rm(tmp, { recursive: true, force: true });
