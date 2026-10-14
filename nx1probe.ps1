<#
  Launch IW4L headless, capture the log, and ALWAYS kill the client.

  Never leaves a process running: the client is stopped in a `finally` block, so a
  timeout or an error still reaps it. Every autonomous test run goes through here
  rather than Start-Process, which is how a client got left running overnight.

    .\nx1probe.ps1                 # default: lobby, start match, join, 100s
    .\nx1probe.ps1 -Seconds 150   # longer settle
    .\nx1probe.ps1 -Cmds "ui_create_lobby;ui_start_match;cl_join;player_spawn"
#>
param(
  [int]$Seconds = 100,
  [string]$Cmds = 'ui_create_lobby;ui_start_match;cl_join'
)

$ErrorActionPreference = 'Continue'
$repo = 'X:\AI\Projects\iw4l'
$exe  = Join-Path $repo 'target\release\iw4l.exe'
$log  = Join-Path $repo 'target\release\iw4l-artifacts\logs\latest.log'
$out  = 'C:\Users\Jett Owens\AppData\Local\Temp\opencode\probe.log'

$proc = $null
try {
  $env:IW4L_GAMES = 'X:\AI\Projects\iw4l-games'
  # The class store overrides --cmds, so a stale profile silently wins.
  # Keep the profile: it holds classes.txt, and spawn <name> needs a class to exist.

  $proc = Start-Process -FilePath $exe -ArgumentList 'menu','--cmds',$Cmds `
    -WorkingDirectory (Join-Path $repo 'target\release') -PassThru -NoNewWindow
  Write-Output "launched pid=$($proc.Id); waiting ${Seconds}s"
  Start-Sleep -Seconds $Seconds
}
finally {
  # Always reap the client, whatever happened above.
  if ($proc -and -not $proc.HasExited) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Write-Output "stopped pid=$($proc.Id)"
  }
  Get-Process -Name iw4l -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}

if (Test-Path $log) {
  Copy-Item $log $out -Force
  Write-Output "log -> $out"
}
# `latest.log` rotates during a run, so it can be the fresh empty file by the time
# the client is stopped. The match log is the largest one written in this window.
$dir = Split-Path $log
$big = Get-ChildItem $dir -Filter '*.log' -File -ErrorAction SilentlyContinue |
  Where-Object { $_.LastWriteTime -gt (Get-Date).AddMinutes(-15) -and $_.Length -gt 50KB } |
  Sort-Object Length -Descending | Select-Object -First 1
if ($big) {
  Copy-Item $big.FullName $out -Force
  Write-Output "log -> $out (from $($big.Name), $([math]::Round($big.Length/1KB))KB)"
}
if (Get-Process -Name iw4l -ErrorAction SilentlyContinue) {
  Write-Output 'WARNING: an iw4l process is still running'
} else {
  Write-Output 'no iw4l process running'
}
