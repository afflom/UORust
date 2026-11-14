# Install the uorust CLI on Windows from a GitHub release of afflom/UORust.
#
#   powershell -ExecutionPolicy Bypass -c "irm https://afflom.github.io/UORust/install.ps1 | iex"
#
# Downloads uorust-<target>.zip, checks its SHA-256 against the release's
# SHA256SUMS, and installs uorust.exe. Settings, all optional:
#
#   $env:UORUST_VERSION      the release to install, e.g. 0.2.0 (default: the latest)
#   $env:UORUST_INSTALL_DIR  where uorust.exe goes (default: %LOCALAPPDATA%\Programs\uorust\bin),
#                            added to the user PATH when it is not on it
#   $env:UORUST_REPOSITORY   the GitHub repository (default: afflom/UORust)
#   $env:UORUST_ATTESTATION  1 to also verify the archive's build provenance with
#                            `gh attestation verify` (requires the GitHub CLI)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repository = if ($env:UORUST_REPOSITORY) { $env:UORUST_REPOSITORY } else { 'afflom/UORust' }
$installDir = if ($env:UORUST_INSTALL_DIR) { $env:UORUST_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\uorust\bin' }
$version = if ($env:UORUST_VERSION) { $env:UORUST_VERSION.TrimStart('v') } else { '' }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($arch) {
  'X64' { $target = 'x86_64-pc-windows-msvc' }
  'Arm64' { $target = 'x86_64-pc-windows-msvc' } # runs under Windows' x64 emulation
  default { throw "uorust-install: no release archive for $arch; build from source: cargo install --locked uorust" }
}
$archive = "uorust-$target.zip"
$base = if ($version) { "https://github.com/$repository/releases/download/v$version" } else { "https://github.com/$repository/releases/latest/download" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("uorust-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
  Write-Host "uorust-install: fetching SHA256SUMS from $base"
  Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile (Join-Path $work 'SHA256SUMS')
  # A line is "<hex>  <name>" or "<hex> *<name>" (binary mode).
  $expected = $null
  foreach ($line in Get-Content (Join-Path $work 'SHA256SUMS')) {
    $fields = $line -split '\s+', 2
    if ($fields.Count -eq 2 -and $fields[1].TrimStart('*') -eq $archive) { $expected = $fields[0].ToLowerInvariant() }
  }
  if (-not $expected) { throw "uorust-install: the release has no $archive" }

  Write-Host "uorust-install: fetching $archive"
  $zip = Join-Path $work $archive
  Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $zip
  $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
  if ($actual -ne $expected) { throw "uorust-install: SHA-256 mismatch for ${archive}: expected $expected, got $actual" }
  Write-Host "uorust-install: SHA-256 $actual matches SHA256SUMS"

  if ($env:UORUST_ATTESTATION -eq '1') {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'uorust-install: UORUST_ATTESTATION=1 needs the GitHub CLI (gh)' }
    & gh attestation verify $zip --repo $repository | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "uorust-install: build provenance of $archive does not verify" }
    Write-Host "uorust-install: build provenance verified: signed by ${repository}'s release workflow"
  }

  Expand-Archive -Path $zip -DestinationPath $work -Force
  New-Item -ItemType Directory -Force -Path $installDir | Out-Null
  Copy-Item -Force (Join-Path $work "uorust-$target\uorust.exe") (Join-Path $installDir 'uorust.exe')
  $installed = & (Join-Path $installDir 'uorust.exe') --version
  Write-Host "uorust-install: installed $installed to $installDir\uorust.exe"

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not (($userPath -split ';') -contains $installDir)) {
    $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "uorust-install: added $installDir to the user PATH; open a new shell to run uorust by name"
  }
} finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
