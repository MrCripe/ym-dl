#!/usr/bin/env node
/**
 * Rewrites a release body from CHANGELOG.md.
 *
 * The body used to be a static block in the workflow, so every release said the
 * same thing and a user could not tell why 1.2.0 was worth installing. This takes
 * the `## <version>` section of the changelog, adds the file list at the end, and
 * writes it to the release.
 *
 * Usage: node scripts/release-notes.mjs <owner/repo> <tag>
 */

import { execFileSync } from "node:child_process";
import { readFileSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [repo, tag] = process.argv.slice(2);
if (!repo || !tag) {
  console.error("usage: release-notes.mjs <owner/repo> <tag>");
  process.exit(1);
}

const version = tag.replace(/^v/, "");

function gh(args) {
  return execFileSync("gh", args, { encoding: "utf8", env: process.env });
}

/** The changelog section for one version, without its heading. */
function sectionFor(text, version) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line.trim() === `## ${version}`);
  if (start === -1) return null;
  const body = [];
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^## /.test(lines[index])) break;
    body.push(lines[index]);
  }
  return body.join("\n").trim();
}

const changelog = readFileSync("CHANGELOG.md", "utf8");
const notes = sectionFor(changelog, version);

const files = `
---

### Какие файлы скачивать

| Что у вас | Файл |
| --- | --- |
| Windows 10/11, установить | \`ym-dl_${version}_windows_x64-setup.exe\` |
| Windows, без установки | \`ym-dl_${version}_windows_x64-portable.zip\` — распаковать и запустить |
| macOS, Apple Silicon | \`ym-dl_${version}_macos_aarch64.dmg\` |
| Linux, без установки | \`ym-dl_${version}_linux_amd64-portable.tar.gz\` — внутри AppImage со всеми библиотеками |
| Linux, AppImage отдельно | \`ym-dl_${version}_linux_amd64.AppImage\` |
| Linux, пакет | \`ym-dl_${version}_linux_amd64.deb\` |

Установленное приложение обновляется само: когда выходит новая версия, внизу
экрана появляется кнопка «Обновить». Проверить вручную можно в «Настройки →
Обновления → Проверить».

Ничего доустанавливать не нужно: FLAC, AAC и MP3 приложение разбирает само.
`;

const body = notes
  ? `## Что нового в ${version}\n\n${notes}\n${files}`
  : `Версия ${version}.\n${files}`;

const dir = mkdtempSync(join(tmpdir(), "ym-dl-notes-"));
const path = join(dir, "notes.md");
writeFileSync(path, body);

gh(["release", "edit", tag, "--repo", repo, "--notes-file", path]);
console.log(`release notes for ${tag} written (${notes ? "from CHANGELOG" : "no changelog entry"})`);
