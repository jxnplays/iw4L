<#
  Remove build output and scratch that a fresh build makes redundant.

  Run after every release build. The rule is: keep what the next build or the next
  test needs, delete what only exists because something was built earlier.

  Kept:
    target\release\iw4l.exe        the build under test
    target\release\iw4l-artifacts asset cache, logs, profile (never clear wgsl)
    target\release\deps            so the next release build links fast

  Removed:
    target\debug                   cargo check cache. Regenerates in a few minutes;
                                   this is the single largest thing on the drive
    target\release\incremental     never useful across a clean rebuild
    captures\*                     old capture runs, newest few kept

  Usage:
    .\clean.ps1              normal sweep
    .\clean.ps1 -KeepCaptures 3
#>
param(
  [int]$KeepCaptures = 2,
  [switch]$KeepDebug
)

$ErrorActionPreference = 'Continue'
$repo = 'X:\AI\Projects\iw4l'

$before = (Get-PSDrive X).Free
Write-Output ("free before: {0} GB" -f [math]::Round($before / 1GB, 1))

# Never touch these.
$protect = @(
  (Join-Path $repo 'target\release\iw4l.exe'),
  (Join-Path $repo 'target\release\iw4l-artifacts'),
  (Join-Path $repo 'target\release\deps')
)

function Remove-Tree($path, $label) {
  if (-not (Test-Path $path)) { return }
  foreach ($keep in $protect) {
    if ($path.StartsWith($keep)) {
      Write-Output "  skip $label (protects $keep)"
      return
    }
  }
  $size = (Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue |
           Measure-Object Length -Sum).Sum
  Remove-Item $path -Recurse -Force -ErrorAction SilentlyContinue
  $state = if (Test-Path $path) { 'PARTIAL' } else { 'removed' }
  Write-Output ("  {0,-8} {1,7} GB  {2}" -f $state, [math]::Round($size / 1GB, 2), $label)
}

if (-not $KeepDebug) {
  Remove-Tree (Join-Path $repo 'target\debug') 'target\debug'
}
Remove-Tree (Join-Path $repo 'target\release\incremental') 'target\release\incremental'

# Old capture runs. Keep the newest few for comparison.
$captures = Join-Path $repo 'captures'
if (Test-Path $captures) {
  Get-ChildItem $captures -Directory -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -Skip $KeepCaptures |
    ForEach-Object { Remove-Tree $_.FullName "captures\$($_.Name)" }
}

# Stray rotated logs; keep the newest few.
$logs = Join-Path $repo 'target\release\iw4l-artifacts\logs'
if (Test-Path $logs) {
  Get-ChildItem $logs -Filter '*.log' -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -Skip 4 |
    ForEach-Object {
      Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
      Write-Output "  removed  $($_.Name)"
    }
}

$after = (Get-PSDrive X).Free
Write-Output ("free after:  {0} GB  (freed {1} GB)" -f [math]::Round($after / 1GB, 1), [math]::Round(($after - $before)/1GB, 1))

# Prove the build under test survived.
$exe = Join-Path $repo 'target\release\iw4l.exe'
if (Test-Path $exe) {
  Write-Output ("exe intact: {0} MB, built {1}" -f [math]::Round((Get-Item $exe).Length/1MB, 1), (Get-Item $exe).LastWriteTime)
} else {
  Write-Output 'WARNING: target\release\iw4l.exe is missing'
}