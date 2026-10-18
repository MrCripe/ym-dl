# Собирает ym-dl под Windows: установщик .msi и .exe (NSIS).
# Запуск из корня репозитория: .\scripts\build-windows.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path $cargo) { $env:Path = "$cargo;$env:Path" }

    foreach ($tool in @('node', 'npm', 'cargo')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
            throw "Не найден $tool. Установите Node.js и Rust (rustup) и повторите."
        }
    }

    if (-not (Test-Path 'node_modules')) {
        Write-Host 'Устанавливаю зависимости фронтенда…' -ForegroundColor Cyan
        npm install --no-audit --no-fund
    }

    Write-Host 'Собираю приложение…' -ForegroundColor Cyan
    npm run app:build
    if ($LASTEXITCODE -ne 0) { throw "Сборка завершилась с кодом $LASTEXITCODE" }

    $bundle = Join-Path $root 'src-tauri\target\release\bundle'
    Get-ChildItem -Path $bundle -Recurse -Include *.msi, *.exe |
        Select-Object FullName, @{n = 'МиБ'; e = { [math]::Round($_.Length / 1MB, 1) } } |
        Format-Table -AutoSize
}
finally {
    Pop-Location
}
