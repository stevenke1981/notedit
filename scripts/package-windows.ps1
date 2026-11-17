param([switch]$SkipBuild, [string]$IronSetup)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if (-not $SkipBuild) {
        cargo build --release --workspace --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    }
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    if (-not $version) { throw 'Could not read package version.' }

    # IronSetup: -IronSetup path, else PATH, else ~/.cargo/bin
    $compiler = @(
        $IronSetup,
        (Get-Command ironsetup.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        "$env:USERPROFILE\.cargo\bin\ironsetup.exe"
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
    if (-not $compiler) { throw 'ironsetup.exe was not found. Pass -IronSetup <path> or run: cargo install --locked --git https://github.com/stevenke1981/ironsetup ironsetup' }

    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Redist.14.Latest -property installationPath
    if (-not $vs) { throw 'Visual C++ x64 redistributable files were not found.' }
    $runtime = Get-ChildItem -Path "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll" |
        Sort-Object { [version]($_.Directory.Parent.Parent.Name) } -Descending | Select-Object -First 1
    if (-not $runtime) { throw 'Visual C++ x64 vcruntime140.dll was not found.' }

    & $compiler compile 'installer\notedit.iss' --define "AppVersion=$version" --define "RuntimeDir=$($runtime.DirectoryName)"
    if ($LASTEXITCODE -ne 0) { throw 'IronSetup compilation failed.' }
    $out = "target\release\installer\NotEdit-$version-windows-x64-setup.exe"
    if (-not (Test-Path -LiteralPath $out)) { throw "Expected installer was not produced: $out" }
    $h = Get-FileHash $out -Algorithm SHA256
    Write-Host "$($h.Hash)  $out"
    $h
} finally {
    Pop-Location
}
