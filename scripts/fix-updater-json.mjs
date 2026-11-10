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
import { mkdtempSync, writeFileSync } from "node:fs";
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

// gh names the uploaded asset after the file, so the file has to be called
// exactly `latest.json` — a temp name like `latest-v1.1.1.json` becomes a second,
// useless asset and the real manifest stays stale.
const outDir = mkdtempSync(join(tmpdir(), "ym-dl-manifest-"));
const out = join(outDir, "latest.json");
writeFileSync(out, JSON.stringify(manifest, null, 2));

// `--clobber` alone proved unreliable here: it left the old asset in place next
// to the new one. Deleting first makes the replacement unambiguous.
gh(["api", "-X", "DELETE", `repos/${repo}/releases/assets/${manifestAsset.id}`]);
gh(["release", "upload", tag, out, "--repo", repo]);
console.log(`latest.json обновлён: ${changed} адрес(ов) заменены на download-ссылки`);
