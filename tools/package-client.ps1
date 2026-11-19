# Susun folder client stream Rust untuk dibundel installer aplikasi CloudPC: stream-rs\target\package\stream-rs\
#   cloudpc-stream-client.exe (release) + avcodec-62.dll + avutil-60.dll (FFmpeg ramping dari CI) + SDL3.dll + lisensi.
#   powershell -ExecutionPolicy Bypass -File E:\cloudpc\stream-rs\tools\package-client.ps1 [-FFmpegDir <folder DLL ramping>]
# Tanpa -FFmpegDir dipakai target\deps\ffmpeg-slim (hasil unduhan artifact CI "ffmpeg-slim-n8.0.1").
# Folder ini dibaca client-rs\build.ps1 (disalin ke dist\CloudPC-Rust\stream-rs). Aplikasi CloudPC memakai client ini bila ada.
param([string]$FFmpegDir = '', [switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
if (-not $FFmpegDir) { $FFmpegDir = Join-Path $root 'target\deps\ffmpeg-slim' }
foreach ($need in 'avcodec-62.dll', 'avutil-60.dll') {
    if (-not (Test-Path (Join-Path $FFmpegDir $need))) { throw "$need tidak ada di $FFmpegDir (unduh artifact CI ffmpeg-slim dulu)" }
}
$sdl = Get-ChildItem (Join-Path $root 'target\deps') -Recurse -Filter SDL3.dll -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $sdl) { throw "SDL3.dll tidak ada di target\deps (rilis resmi libsdl-org/SDL, SDL3-<versi>-win32-x64.zip)" }

if (-not $SkipBuild) {
    cargo build --release -p cloudpc-stream-client
    if ($LASTEXITCODE -ne 0) { throw "cargo build gagal" }
}
$out = Join-Path $root 'target\package\stream-rs'
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item (Join-Path $root 'target\release\cloudpc-stream-client.exe') $out
# Semua DLL dari build ramping (avcodec, avutil, dan DLL MinGW yang ikut dibutuhkan, bila ada).
Get-ChildItem $FFmpegDir -Filter *.dll | Copy-Item -Destination $out
Copy-Item $sdl.FullName $out
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $out 'LICENSE-cloudpc-stream.txt')
foreach ($extra in 'FFMPEG-LICENSE.txt', 'FFMPEG-SOURCE.txt') {
    if (Test-Path (Join-Path $FFmpegDir $extra)) { Copy-Item (Join-Path $FFmpegDir $extra) $out }
}
$sdlLicense = Join-Path $sdl.DirectoryName 'LICENSE.txt'
if (Test-Path $sdlLicense) { Copy-Item $sdlLicense (Join-Path $out 'SDL3-LICENSE.txt') }
@"
Client stream CloudPC (pengganti Moonlight), lisensi GPL-3.0 (LICENSE-cloudpc-stream.txt).
Source code (GPL-3.0): https://github.com/HusniHeartnet/cloudpc-stream-rs
Komponen: FFmpeg (LGPL-2.1, FFMPEG-LICENSE.txt), SDL3 (zlib, SDL3-LICENSE.txt).
"@ | Set-Content (Join-Path $out 'README.txt') -Encoding utf8
Get-ChildItem $out | Select-Object Name, @{n = 'MB'; e = { '{0:N1}' -f ($_.Length / 1MB) } } | Format-Table -AutoSize | Out-String
"Total {0:N1} MB" -f ((Get-ChildItem $out | Measure-Object Length -Sum).Sum / 1MB)
