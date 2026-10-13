<#
  Capture the IW4L window on an interval, then close the client.

  Full-screen capture records whatever happens to be on top -- which is how three
  rounds recorded the IDE instead of the game. This targets the game window by
  process and uses PrintWindow, so the frames are the game even when something
  else has focus.

  Capture and shutdown are one script: the client cannot be left running.

    .\capture_and_close.ps1 -Seconds 60 -Every 5
#>
param(
  [int]$Seconds = 60,
  [int]$Every = 5,
  [string]$Tag = 'nx1'
)

$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
public class Win {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static System.Drawing.Rectangle RectOf(IntPtr h) {
    RECT r; GetWindowRect(h, out r);
    return new System.Drawing.Rectangle(0, 0, r.Right - r.Left, r.Bottom - r.Top);
  }
}
'@ -ReferencedAssemblies System.Drawing, System.Windows.Forms

function Get-GameWindow {
  $p = Get-Process -Name iw4l -ErrorAction SilentlyContinue |
       Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($p) { return $p.MainWindowHandle }
  # MainWindowHandle can be 0 before the window is realised; fall back to the
  # first top-level window the process owns.
  $p = Get-Process -Name iw4l -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $p) { return [IntPtr]::Zero }
  return $p.MainWindowHandle
}

$proc = Get-Process -Name iw4l -ErrorAction SilentlyContinue
if (-not $proc) {
  Write-Output 'ERROR: iw4l is not running - nothing to capture'
  exit 1
}

Start-Sleep -Milliseconds 500
$hwnd = Get-GameWindow
if ($hwnd -eq [IntPtr]::Zero) {
  Write-Output 'ERROR: could not find the iw4l window'
  Get-Process -Name iw4l -ErrorAction SilentlyContinue | Stop-Process -Force
  exit 1
}
[void][Win]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 700

$rect = [Win]::RectOf($hwnd)
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$dir = Join-Path 'X:\AI\Projects\iw4l' "captures\$Tag-$stamp"
New-Item -ItemType Directory -Path $dir -Force | Out-Null
Write-Output "capturing iw4l window $($rect.Width)x$($rect.Height) every ${Every}s for ${Seconds}s"
Write-Output "-> $dir"
Write-Output 'GO'

$deadline = (Get-Date).AddSeconds($Seconds)
$i = 0
while ((Get-Date) -lt $deadline) {
  $i++
  $bmp = New-Object System.Drawing.Bitmap $rect.Width, $rect.Height
  $gfx = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $gfx.GetHdc()
  [void][Win]::PrintWindow($hwnd, $hdc, 2)
  $gfx.ReleaseHdc($hdc)
  $gfx.Dispose()
  $path = Join-Path $dir ("{0:d3}-{1}.png" -f $i, (Get-Date -Format 'HHmmss'))
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output "  $path"
  Start-Sleep -Seconds $Every
}

Write-Output "done: $i frames in $dir"

Get-Process -Name iw4l -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
if (Get-Process -Name iw4l -ErrorAction SilentlyContinue) {
  Write-Output 'WARNING: iw4l is still running'
} else {
  Write-Output 'client closed'
}