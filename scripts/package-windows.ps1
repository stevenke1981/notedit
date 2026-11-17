param([switch]$SkipBuild)

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
    $compiler = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
    if (-not $compiler) { throw 'Inno Setup compiler ISCC.exe was not found.' }

    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Redist.14.Latest -property installationPath
    if (-not $vs) { throw 'Visual C++ x64 redistributable files were not found.' }
    $runtime = Get-ChildItem -Path "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll" |
        Sort-Object { [version]($_.Directory.Parent.Parent.Name) } -Descending | Select-Object -First 1
    if (-not $runtime) { throw 'Visual C++ x64 vcruntime140.dll was not found.' }
    & $compiler "/DAppVersion=$version" "/DRuntimeDir=$($runtime.DirectoryName)" 'installer\notedit.iss'
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }
    Get-FileHash "target\release\installer\NotEdit-$version-windows-x64-setup.exe" -Algorithm SHA256
} finally {
    Pop-Location
}
