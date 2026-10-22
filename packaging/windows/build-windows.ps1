# Build VeloMov on Windows itself (PowerShell):
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build-windows.ps1
# Needs: Rust (https://rustup.rs, with the "Desktop development with C++" build tools it asks for).
# Makes dist\VeloMov-<version>-windows-x64\ (runs from there), the portable zip, and the installer
# when Inno Setup 6 is installed (https://jrsoftware.org/isinfo.php).
# FFmpeg (BtbN build) and whisper.cpp (official release) are downloaded once into dist\_downloads.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$Root = (Get-Location).Path
$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
$Dl = Join-Path $Root "dist\_downloads"
New-Item -ItemType Directory -Force -Path $Dl | Out-Null

function Fetch($url, $name) {
    $out = Join-Path $Dl $name
    if (-not (Test-Path $out)) {
        Write-Host "==> downloading $name"
        Invoke-WebRequest -Uri $url -OutFile "$out.part" -UseBasicParsing
        Move-Item "$out.part" $out
    }
    return $out
}

Write-Host "==> cargo build --release"
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$ff = Fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip" "ffmpeg-win.zip"
$wh = Fetch "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip" "whisper-win.zip"

$Stage = Join-Path $Root "dist\VeloMov-$Version-windows-x64"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path "$Stage\ffmpeg", "$Stage\whisper" | Out-Null
Copy-Item "target\release\velomov.exe" $Stage

$tmp = Join-Path $env:TEMP ("velomov-" + [guid]::NewGuid())
Expand-Archive $ff -DestinationPath "$tmp\ff"
Expand-Archive $wh -DestinationPath "$tmp\wh"
$ffbin = Get-ChildItem "$tmp\ff" -Directory | Select-Object -First 1
Copy-Item "$($ffbin.FullName)\bin\*" "$Stage\ffmpeg\"
Copy-Item "$tmp\wh\Release\whisper-cli.exe", "$tmp\wh\Release\whisper.dll", "$tmp\wh\Release\ggml*.dll" "$Stage\whisper\"
Remove-Item -Recurse -Force $tmp
Copy-Item LICENSE "$Stage\LICENSE.txt"
Copy-Item packaging\windows\LEIA-ME-WINDOWS.txt "$Stage\LEIA-ME.txt"
Write-Host "==> $Stage (the official whisper.cpp build needs the Visual C++ Redistributable)"

# portable zip
$Port = Join-Path $env:TEMP "VeloMov-$Version-portable"
if (Test-Path $Port) { Remove-Item -Recurse -Force $Port }
Copy-Item -Recurse $Stage $Port
"VeloMov portable: projects, settings and caches are kept in the ""data"" folder next to velomov.exe." | Set-Content "$Port\portable.txt"
$Zip = Join-Path $Root "dist\VeloMov-$Version-windows-x64-portable.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path $Port -DestinationPath $Zip
Remove-Item -Recurse -Force $Port
Write-Host "==> $Zip"

# installer
$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "${env:ProgramFiles}\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) {
    & $iscc /Q "/DAppVersion=$Version" "/DSourceDir=$Stage" "/DOutputDir=$Root\dist" "/DRepoDir=$Root" "packaging\windows\velomov.iss"
    Write-Host "==> dist\VeloMov-$Version-windows-x64-setup.exe"
} else {
    Write-Host "Inno Setup 6 not found: skipped the installer (the portable zip is ready)."
}
