# Builds release exes for x64 and arm64 and packs both into one MSI:
#   target\dist\Matejko-<version>.msi
#   target\dist\matejko-<version>-x64.exe
#   target\dist\matejko-<version>-arm64.exe
#
# Needs Rust with the MSVC x64 and arm64 build tools, and WiX Toolset 3.14.

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

function Invoke-Checked {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Exe failed with exit code $LASTEXITCODE."
    }
}

$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'matejko' }
$version = $package.version
# MSI ProductVersion takes only major.minor.build numbers.
$msiVersion = ($version -split '[-+]')[0]

$targets = [ordered]@{
    x64   = 'x86_64-pc-windows-msvc'
    arm64 = 'aarch64-pc-windows-msvc'
}

Invoke-Checked rustup (@('target', 'add') + $targets.Values)

$dist = 'target\dist'
$wixOut = 'target\wix'
New-Item -ItemType Directory -Force $dist, $wixOut | Out-Null

$exes = @{}
foreach ($platform in $targets.Keys) {
    $target = $targets[$platform]
    Invoke-Checked cargo @('build', '--release', '--locked', '--target', $target)
    $exe = Resolve-Path "target\$target\release\matejko.exe"
    $exes[$platform] = $exe
    Copy-Item $exe "$dist\matejko-$version-$platform.exe"
}

# The installer's license page needs RTF. Each LICENSE paragraph becomes one.
$paragraphs = (Get-Content -Raw LICENSE) -split '\r?\n\s*\r?\n' | ForEach-Object {
    $text = ($_.Trim() -replace '\r?\n', ' ') -replace '([\\{}])', '\$1'
    "$text\par"
}
$license = "$wixOut\License.rtf"
Set-Content -Encoding ascii $license ("{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\fs18`r`n" + ($paragraphs -join "`r`n\par`r`n") + "`r`n}")

if (-not $env:WIX -or -not (Test-Path "$env:WIX\bin\candle.exe")) {
    throw 'WiX Toolset 3.14 not found. Install it from https://github.com/wixtoolset/wix3/releases.'
}
$wixBin = Join-Path $env:WIX 'bin'

Invoke-Checked "$wixBin\candle.exe" @(
    '-nologo', '-arch', 'x64', '-ext', 'WixUIExtension',
    "-dVersion=$msiVersion",
    "-dX64Exe=$($exes['x64'])",
    "-dArm64Exe=$($exes['arm64'])",
    "-dIcon=$(Resolve-Path assets\icon.ico)",
    "-dLicense=$(Resolve-Path $license)",
    '-out', "$wixOut\",
    'installer\matejko.wxs'
)

$msi = "$dist\Matejko-$version.msi"
# ICE30: both exes target matejko.exe in one folder; their conditions keep
# them apart. ICE57: the HKMU shortcut keypath is right for a package that
# installs per user or per machine, which ICE57 does not model.
Invoke-Checked "$wixBin\light.exe" @(
    '-nologo', '-ext', 'WixUIExtension', '-cultures:en-us',
    '-sice:ICE30', '-sice:ICE57', '-spdb',
    '-out', $msi,
    "$wixOut\matejko.wixobj"
)

Write-Host "Built $(Resolve-Path $msi)"
