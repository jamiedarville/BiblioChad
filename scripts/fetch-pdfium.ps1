# Download PDFium for Windows into src-tauri\resources\pdfium\.
# Usage: .\scripts\fetch-pdfium.ps1 [-Platform win-arm64]
param([string]$Platform = "win-arm64", [string]$Version = "latest")
$ErrorActionPreference = "Stop"
$dest = Join-Path $PSScriptRoot "..\src-tauri\resources\pdfium"
if ($Version -eq "latest") {
  $url = "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-$Platform.tgz"
} else {
  $url = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$Version/pdfium-$Platform.tgz"
}
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("pdfium-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp, $dest | Out-Null
Invoke-WebRequest -Uri $url -OutFile "$tmp\pdfium.tgz"
tar -xzf "$tmp\pdfium.tgz" -C $tmp
Copy-Item "$tmp\bin\pdfium.dll" $dest -Force
if (Test-Path "$tmp\LICENSE") { Copy-Item "$tmp\LICENSE" "$dest\PDFIUM_LICENSE.txt" -Force }
Remove-Item -Recurse -Force $tmp
Write-Host "PDFium ($Platform) -> $dest"
