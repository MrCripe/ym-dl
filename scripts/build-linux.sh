#!/usr/bin/env bash
# Собирает ym-dl под Linux: .deb и .AppImage.
# Запуск из корня репозитория: ./scripts/build-linux.sh
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in node npm cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Не найден $tool. Установите Node.js и Rust (rustup) и повторите." >&2
        exit 1
    fi
done

missing=$(pkg-config --exists webkit2gtk-4.1 2>/dev/null && echo ok || echo no)
if [ "$missing" != "ok" ]; then
    cat >&2 <<'EOF'
Не найдены библиотеки WebKitGTK. Установите их:

  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
EOF
    exit 1
fi

[ -d node_modules ] || npm install --no-audit --no-fund

npm run app:build

find src-tauri/target/release/bundle -type f \( -name '*.deb' -o -name '*.AppImage' \) \
    -printf '%p  %k KiB\n'
