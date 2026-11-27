// Renames release assets to one scheme: ym-dl_<version>_<os>_<arch>.<ext>.
//
// The bundler names files after each platform's own convention — `x64-setup.exe`,
// `aarch64.dmg`, `amd64.AppImage` — which reads fine to a packager and badly to
// someone picking a download. Renaming through the API keeps the asset id, so
// everything that is rewritten by fix-updater-json.mjs afterwards still resolves.
//
// The label is set too: an asset uploaded with a label keeps showing that label on
// the release page however the file itself is renamed, so renaming the name alone
// left the old bundler name in front of the user.
//
// The .msi is dropped here as well: the .exe installer replaces an installed
// build in place, while an .msi needs the old one removed first, and two Windows
// installers in one release only make the choice harder.
//
// Usage: node scripts/rename-assets.mjs <owner/repo> <tag>
import { execFileSync } from "node:child_process";

const [repo, tag] = process.argv.slice(2);
if (!repo || !tag) {
  console.error("usage: rename-assets.mjs <owner/repo> <tag>");
  process.exit(2);
}

const gh = (args, options = {}) =>
  execFileSync("gh", args, { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, ...options });

// Left-hand side is what the bundler produces, right-hand side is what a person
// reads. Order matters only in that the longer suffixes come first.
const NAMES = [
  ["x64-setup.exe", "windows_x64-setup.exe"],
  ["windows-portable.zip", "windows_x64-portable.zip"],
  ["amd64.AppImage", "linux_amd64.AppImage"],
  ["amd64.deb", "linux_amd64.deb"],
  ["linux-portable.tar.gz", "linux_amd64-portable.tar.gz"],
  ["aarch64.dmg", "macos_aarch64.dmg"],
  ["aarch64.app.tar.gz", "macos_aarch64.app.tar.gz"],
];

const release = JSON.parse(gh(["api", `repos/${repo}/releases/tags/${tag}`]));
const assets = release.assets.map((asset) => ({
  id: asset.id,
  name: asset.name,
  label: asset.label ?? "",
}));

const drop = (asset) => /\.msi(\.sig)?$/.test(asset.name);

/** `ym-dl_1.4.2_x64-setup.exe` -> `ym-dl_1.4.2_windows_x64-setup.exe`. */
function mapped(name) {
  const match = /^(.*_\d+\.\d+\.\d+)_(.+)$/.exec(name);
  if (!match) return name;
  const [, head, tail] = match;
  const rule = NAMES.find(([from]) => tail === from);
  // A name that is already in the scheme comes back unchanged, so a second run of
  // this script only fixes the labels it missed the first time.
  return rule ? `${head}_${rule[1]}` : name;
}

let removed = 0;
let patched = 0;

for (const asset of assets) {
  if (drop(asset)) {
    gh(["api", "-X", "DELETE", `repos/${repo}/releases/assets/${asset.id}`]);
    console.log(`удалён ${asset.name}`);
    removed += 1;
    continue;
  }
  // latest.json is the updater manifest and .sig files are deleted by the polish
  // workflow; neither is downloaded by a person.
  if (asset.name === "latest.json" || asset.name.endsWith(".sig")) continue;

  const target = mapped(asset.name);
  const fields = [];
  if (asset.name !== target) fields.push(`name=${target}`);
  if (asset.label && asset.label !== target) fields.push(`label=${target}`);
  if (!fields.length) continue;

  gh([
    "api",
    "-X",
    "PATCH",
    `repos/${repo}/releases/assets/${asset.id}`,
    ...fields.flatMap((field) => ["-f", field]),
  ]);
  console.log(`${asset.label || asset.name} → ${target}`);
  patched += 1;
}

console.log(`переименовано: ${patched}, удалено: ${removed}`);
