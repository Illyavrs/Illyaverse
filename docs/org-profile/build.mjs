// Renders the images used by the GitHub organization profile README: banner, architecture
// diagram, feature tiles and one card per repository. Same glass material and background scene
// as docs/readme/build.mjs. Run from the repository root:  node docs/org-profile/build.mjs
import { chromium } from "playwright";
import sharp from "sharp";
import { mkdir, writeFile, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const assets = pathToFileURL(join(here, "..", "readme", "assets")).href;
const out = join(here, "images");
const W = 1200;

const css = `
@font-face{font-family:Nunito;src:url("${assets}/nunito-latin-wght-normal.woff2");font-weight:200 1000}
*{box-sizing:border-box;margin:0}
html,body{font-family:Nunito,"Microsoft JhengHei",sans-serif;color:#f5f5f5;-webkit-font-smoothing:antialiased}
.stage{position:relative;overflow:hidden;background:#0a0a0c}
.bg{position:absolute;inset:0;background:url("${assets}/background-source.png") var(--pos,center) / cover no-repeat}
.tint{position:absolute;inset:0;background:rgb(8 8 10/var(--dim,.30))}
.glass{position:relative;background:rgb(24 24 27/.55);border:1px solid rgb(255 255 255/.14);border-radius:14px;
  backdrop-filter:blur(14px);-webkit-backdrop-filter:blur(14px);box-shadow:0 12px 36px rgb(0 0 0/.25)}
.pink{color:#f28eb6}
`;
const page = (body, w, h, vars = "") => `<!doctype html><html><head><meta charset="utf-8"><style>${css}
html,body{width:${w}px}</style></head>
<body><div class="stage" style="width:${w}px;height:${h}px;${vars}"><div class="bg"></div><div class="tint"></div>${body}</div></body></html>`;

const arrow = `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>`;

// ---------------------------------------------------------------- banner
const strip = ["Player website", "Rust services", "Game launcher", "Documentation"];
const banner = page(
  `<style>
    .card{position:absolute;left:40px;right:40px;top:40px;bottom:40px;overflow:hidden}
    .copy{position:absolute;left:52px;top:44px;width:660px}
    .kick{font-size:15px;font-weight:800;letter-spacing:.14em;text-transform:uppercase;color:#a8a8a8}
    .word{margin-top:10px;font-size:104px;line-height:1;font-weight:900;letter-spacing:-.04em}
    .desc{margin-top:16px;font-size:20px;line-height:1.55;font-weight:600;color:#d8d8d8;max-width:600px}
    .char{position:absolute;right:30px;top:-40px;height:760px}
    .strip{position:absolute;left:0;right:0;bottom:0;height:58px;display:grid;grid-template-columns:repeat(4,1fr);
           border-top:1px solid rgb(255 255 255/.12);background:rgb(20 20 23/.88)}
    .strip div{display:flex;align-items:center;justify-content:space-between;padding:0 24px;font-size:15px;font-weight:800;color:#e8e8e8}
    .strip div+div{border-left:1px solid rgb(255 255 255/.12)}
    .strip svg{color:#a8a8a8}
  </style>
  <div class="glass card">
    <div class="copy">
      <div class="kick">Illyavrs · open source</div>
      <div class="word">Illya<span class="pink">verse</span></div>
      <p class="desc">Everything needed to run an osu! community server: a customizable website, Rust services for stable and lazer, and a launcher to connect.</p>
    </div>
    <img class="char" src="${assets}/character.png">
    <div class="strip">${strip.map((l) => `<div>${l}${arrow}</div>`).join("")}</div>
  </div>`,
  W,
  460,
  "--pos:center 28%;--dim:.22",
);

// ---------------------------------------------------------------- architecture
const node = (x, y, w, h, title, sub, accent = false) =>
  `<div class="glass n" style="left:${x}px;top:${y}px;width:${w}px;height:${h}px;${accent ? "border-color:#f28eb6;box-shadow:0 0 0 1px #f28eb655,0 12px 36px rgb(0 0 0/.25)" : ""}"><div class="nt">${title}</div><div class="ns">${sub}</div></div>`;
const line = (x1, y1, x2, y2, dash = false) =>
  `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="#f5f5f5" stroke-opacity=".75" stroke-width="2.2" ${dash ? 'stroke-dasharray="7 7"' : 'marker-end="url(#a)"'}/>`;
const architecture = page(
  `<style>
    .panel{position:absolute;left:40px;right:40px;top:40px;bottom:40px}
    .head{position:absolute;left:36px;top:26px;font-size:26px;font-weight:900}
    .head span{display:block;margin-top:4px;font-size:15px;font-weight:600;color:#c8c8c8}
    .n{position:absolute;display:flex;flex-direction:column;justify-content:center;padding:0 22px}
    .nt{font-size:20px;font-weight:900}
    .ns{margin-top:3px;font-size:14px;font-weight:600;color:#c8c8c8}
    .col{position:absolute;top:104px;font-size:12px;font-weight:800;letter-spacing:.14em;text-transform:uppercase;color:#a8a8a8}
    svg.wires{position:absolute;inset:0;overflow:visible}
  </style>
  <div class="glass panel">
    <div class="head">How the pieces fit<span>Every service ships as a container image built by GitHub Actions.</span></div>
    <div class="col" style="left:36px">Players</div>
    <div class="col" style="left:410px">Edge</div>
    <div class="col" style="left:790px">Core</div>
    <svg class="wires" width="1120" height="560" viewBox="0 0 1120 560">
      <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto"><path d="M0 0L10 5L0 10z" fill="#f5f5f5" fill-opacity=".85"/></marker></defs>
      ${line(296, 175, 408, 175)}
      ${line(296, 325, 408, 325)}
      ${line(668, 175, 788, 232)}
      ${line(668, 325, 788, 268)}
      ${line(940, 300, 940, 392)}
      ${line(190, 408, 190, 372, true)}
    </svg>
    ${node(36, 130, 260, 90, "Browser", "Play, browse, manage")}
    ${node(36, 280, 260, 90, "osu! stable / lazer", "Stock game clients")}
    ${node(410, 130, 258, 90, "Serendipity", "Website · Next.js", true)}
    ${node(410, 280, 258, 90, "Resonance", "stable + lazer protocols · Rust", true)}
    ${node(790, 200, 294, 100, "Sanctuary", "Accounts, maps, scores · Rust", true)}
    ${node(790, 394, 294, 90, "PostgreSQL", "Data, media, replays")}
    ${node(36, 408, 632, 90, "Kaleidoscope + Transfiguration", "Launcher and lazer bridge: set up the client on the player's PC")}
  </div>`,
  W,
  640,
  "--pos:center 60%;--dim:.4",
);

// ---------------------------------------------------------------- features
const tiles = [
  ["Players", "Accounts, sessions, email verification, preferred mode"],
  ["Website", "EN / 繁中 / 日本語, themes, online players, friends"],
  ["Profiles", "Avatars, banners, accent colors, rank history"],
  ["Beatmaps", "osu! imports, uploads, previews, ranking requests"],
  ["Scores", "Normal and Relax boards, Top Play, replays"],
  ["Chat", "Channels, private messages, notifications, broadcasts"],
  ["Staff", "Role hierarchy, moderation, audit records"],
  ["Deploy", "Docker Compose with prebuilt GHCR images"],
];
const features = page(
  `<style>
    .grid{position:absolute;left:40px;right:40px;top:40px;bottom:40px;display:grid;grid-template-columns:repeat(4,1fr);grid-template-rows:1fr 1fr;gap:16px}
    .t{padding:22px 24px;display:flex;flex-direction:column;justify-content:center}
    .t b{font-size:22px;font-weight:900}
    .t b::before{content:"";display:inline-block;width:8px;height:8px;margin-right:10px;border-radius:50%;background:#f28eb6;vertical-align:2px}
    .t p{margin-top:8px;font-size:15px;line-height:1.5;font-weight:600;color:#d0d0d0}
  </style>
  <div class="grid">${tiles.map(([t, d]) => `<div class="glass t"><b>${t}</b><p>${d}</p></div>`).join("")}</div>`,
  W,
  400,
  "--pos:center 80%;--dim:.35",
);

// ---------------------------------------------------------------- repository cards
const repos = [
  ["Illyaverse", "The monorepo: deployment, migrations and shared crates.", ["Docker", "PostgreSQL"], "center 20%", true],
  ["Serendipity", "The player website and staff interface.", ["Next.js", "TypeScript"], "center 40%"],
  ["Sanctuary", "The community API: accounts, maps, scores and player data.", ["Rust", "axum"], "center 60%"],
  ["Resonance", "The gateway for the osu! stable and lazer protocols.", ["Rust"], "center 30%"],
  ["Kaleidoscope", "A launcher for stable and lazer with a CLI and desktop app.", ["Rust", "Tauri"], "center 70%"],
  ["Transfiguration", "A managed authentication bridge for stock lazer clients.", ["C#"], "center 50%"],
  ["Docs", "Documentation for players and server operators.", ["Fumadocs"], "center 85%"],
];
const card = ([name, desc, tags, pos, wide]) => {
  const w = wide ? W : 592;
  const h = 176;
  return [
    `repo-${name.toLowerCase()}`,
    page(
      `<style>
        .c{position:absolute;inset:10px;display:flex;align-items:center;gap:22px;padding:0 30px}
        .mark{flex:none;width:68px;height:68px;border-radius:16px;display:grid;place-items:center;font-size:34px;font-weight:900;
              background:#f28eb6;color:#111}
        .name{font-size:26px;font-weight:900;letter-spacing:-.01em}
        .d{margin-top:4px;font-size:15px;line-height:1.45;font-weight:600;color:#d4d4d4}
        .tags{margin-top:10px;display:flex;gap:8px}
        .tags span{padding:2px 12px;border-radius:99px;background:rgb(255 255 255/.12);font-size:13px;font-weight:800;color:#eee}
        .go{margin-left:auto;color:#cfcfcf}
      </style>
      <div class="glass c"><div class="mark">${name[0]}</div>
        <div><div class="name">${name}</div><div class="d">${desc}</div><div class="tags">${tags.map((t) => `<span>${t}</span>`).join("")}</div></div>
        <div class="go">${arrow}</div></div>`,
      w,
      h,
      `--pos:${pos};--dim:.3`,
    ),
    w,
    h,
  ];
};

const jobs = [
  ["banner", banner, W, 460],
  ["architecture", architecture, W, 640],
  ["features", features, W, 400],
  ...repos.map(card).map(([n, html, w, h]) => [n, html, w, h]),
];

await mkdir(out, { recursive: true });
const tmp = join(here, "tmp");
await mkdir(tmp, { recursive: true });
const browser = await chromium.launch();
const ctx = await browser.newContext({ deviceScaleFactor: 2 });
for (const [name, html, w, h] of jobs) {
  const file = join(tmp, `${name}.html`);
  await writeFile(file, html);
  const p = await ctx.newPage();
  await p.setViewportSize({ width: w, height: h });
  await p.goto(pathToFileURL(file).href);
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(300);
  const png = await p.screenshot({ clip: { x: 0, y: 0, width: w, height: h } });
  await sharp(png).webp({ quality: 90 }).toFile(join(out, `${name}.webp`));
  await p.close();
  console.log("wrote", `${name}.webp`);
}
await browser.close();
await rm(tmp, { recursive: true, force: true });
