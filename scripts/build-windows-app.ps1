Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Package = "frontend"
$BinaryName = "frontend.exe"
$OutputBinaryName = "Swanium.exe"
$Target = "x86_64-pc-windows-msvc"
$DistDir = "dist"
$PackageDir = Join-Path $DistDir "package"
$ZipPath = Join-Path $DistDir "swanium-windows.zip"

for ($i = 0; $i -lt $args.Count; $i++) {
    switch ($args[$i]) {
        "--target" {
            if ($i + 1 -ge $args.Count) { throw "missing value for --target" }
            $Target = $args[$i + 1]
            $i++
        }
        "--zip-path" {
            if ($i + 1 -ge $args.Count) { throw "missing value for --zip-path" }
            $ZipPath = $args[$i + 1]
            $i++
        }
        "-h" { Write-Output "Usage: ./scripts/build-windows-app.ps1 [--target TARGET] [--zip-path PATH]"; exit 0 }
        "--help" { Write-Output "Usage: ./scripts/build-windows-app.ps1 [--target TARGET] [--zip-path PATH]"; exit 0 }
        default { throw "unknown argument: $($args[$i])" }
    }
}

rustup target add $Target
cargo build -p $Package --release --target $Target

if (Test-Path $PackageDir) {
    Remove-Item $PackageDir -Recurse -Force
}
New-Item -ItemType Directory -Path $PackageDir | Out-Null
Copy-Item "target/$Target/release/$BinaryName" "$PackageDir/$OutputBinaryName"

$ZipDir = Split-Path -Parent $ZipPath
if ($ZipDir -and -not (Test-Path $ZipDir)) {
    New-Item -ItemType Directory -Path $ZipDir | Out-Null
}
if (Test-Path $ZipPath) {
    Remove-Item $ZipPath -Force
}
Compress-Archive -Path "$PackageDir/*" -DestinationPath $ZipPath

Write-Output "Packaged $ZipPath"
