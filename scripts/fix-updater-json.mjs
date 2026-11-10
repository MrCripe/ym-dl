// Rewrites latest.json so every platform points at the release download URL.
//
// tauri-action writes the manifest while the release is still a draft, and a
// draft has no `browser_download_url` — so it falls back to
// `api.github.com/repos/…/releases/assets/<id>`, which answers with JSON metadata
// instead of the binary. The updater then downloads that JSON and fails the
// signature check. Running after the release is published replaces those with the
// real download links.
//
// Usage: node scripts/fix-updater-json.mjs <owner/repo> <tag>
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [repo, tag] = process.argv.slice(2);
if (!repo || !tag) {
  console.error("usage: node scripts/fix-updater-json.mjs <owner/repo> <tag>");
  process.exit(2);
}

const gh = (args) =>
  execFileSync("gh", args, { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });

const release = JSON.parse(gh(["api", `repos/${repo}/releases/tags/${tag}`]));
const assets = new Map(release.assets.map((asset) => [asset.name, asset]));
const byId = new Map(release.assets.map((asset) => [String(asset.id), asset]));

const manifestAsset = assets.get("latest.json");
if (!manifestAsset) {
  console.error(`latest.json отсутствует в релизе ${tag}`);
  process.exit(1);
}

const manifest = JSON.parse(gh(["api", `repos/${repo}/releases/assets/${manifestAsset.id}`, "-H", "Accept: application/octet-stream"]));

let changed = 0;
for (const [platform, entry] of Object.entries(manifest.platforms ?? {})) {
  // The URL is either the API asset form (`…/releases/assets/<id>`) or already a
  // download link. Both are resolved back to an asset, so the script can be run
  // again without damage.
  const tail = entry.url.split("/").pop();
  const asset = byId.get(tail) ?? assets.get(tail);
  if (!asset) {
    console.error(`${platform}: ассет ${tail} не найден в релизе`);
    process.exit(1);
  }
  if (entry.url !== asset.browser_download_url) {
    entry.url = asset.browser_download_url;
    changed += 1;
  }
}

if (changed === 0) {
  console.log("latest.json уже указывает на download-ссылки");
  process.exit(0);
}

const out = join(tmpdir(), `latest-${tag.replace(/[^\w.-]/g, "_")}.json`);
writeFileSync(out, JSON.stringify(manifest, null, 2));
gh(["release", "upload", tag, out, "--repo", repo, "--clobber"]);
console.log(`latest.json обновлён: ${changed} адрес(ов) заменены на download-ссылки`);
