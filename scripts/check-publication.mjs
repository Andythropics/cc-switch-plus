import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

const json = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const pkg = json("package.json");
const tauri = json("src-tauri/tauri.conf.json");
const cargo = fs.readFileSync("src-tauri/Cargo.toml", "utf8");
const lock = fs.readFileSync("src-tauri/Cargo.lock", "utf8");

assert.equal(pkg.name, "cc-switch-plus");
assert.equal(pkg.license, "MIT");
assert.equal(tauri.version, pkg.version);
assert.equal(cargo.match(/^version = "([^"]+)"/m)?.[1], pkg.version);
assert.equal(
  lock.match(/name = "cc-switch"\nversion = "([^"]+)"/)?.[1],
  pkg.version,
);
assert.equal(tauri.productName, "CC Switch Plus");
assert.equal(tauri.identifier, "com.ccswitch.plus.desktop");
assert.equal(tauri.bundle.createUpdaterArtifacts, false);
assert.equal(tauri.plugins.updater, undefined);
assert.ok(
  fs.readFileSync("LICENSE", "utf8").includes("Copyright (c) 2025 Jason Young"),
);
assert.ok(fs.readFileSync("LICENSE", "utf8").includes("Andythropics"));
assert.ok(
  !JSON.stringify(json("src-tauri/capabilities/default.json")).includes(
    "updater:",
  ),
);

const docs = [
  "README.md",
  "README_ZH.md",
  "CONTRIBUTING.md",
  "SECURITY.md",
  "CODE_OF_CONDUCT.md",
  "ROADMAP.md",
  "CHANGELOG_PLUS.md",
  "docs/INSTALL.md",
  "docs/RELEASING.md",
  "docs/LAUNCH.md",
  "docs/releases/initial-preview.md",
];
let checked = 0;
for (const file of docs) {
  const source = fs.readFileSync(file, "utf8").replace(/```[\s\S]*?```/g, "");
  const links = [
    ...Array.from(
      source.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g),
      (m) => m[1],
    ),
    ...Array.from(source.matchAll(/(?:src|href)="([^"]+)"/g), (m) => m[1]),
  ];
  for (const link of links) {
    if (/^(?:https?:|mailto:|#)/.test(link)) continue;
    const target = decodeURIComponent(link.split("#")[0]);
    assert.ok(
      fs.existsSync(path.resolve(path.dirname(file), target)),
      `${file}: missing ${target}`,
    );
    checked++;
  }
}
console.log(
  `Publication checks passed: version ${pkg.version}, MIT attribution, updater isolation, ${checked} local links.`,
);
