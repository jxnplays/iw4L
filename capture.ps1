<#
  Capture the screen on an interval, for a duration.

  Lets the NX1 viewmodel be reviewed from recorded frames instead of one
  screenshot at a time.

    .\capture.ps1 -Seconds 60 -Every 5                  # 12 frames
    .\capture.ps1 -Seconds 600 -Every 5 -Tag playtest    # 120 frames, tagged

  Writes PNGs newest-last into .\captures\<tag>-<timestamp>\. Always prints the
  folder, and leaves nothing running behind it.
#>
param(
  [int]$Seconds = 60,
  [int]$Every = 5,
  [string]$Tag = 'nx1'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

$repo = 'X:\AI\Projects\iw4l'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$dir = Join-Path $repo "captures\$Tag-$stamp"
New-Item -ItemType Directory -Path $dir -Force | Out-Null

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Write-Output "capturing $($bounds.Width)x$($bounds.Height) every ${Every}s for ${Seconds}s"
Write-Output "-> $dir"

$deadline = (Get-Date).AddSeconds($Seconds)
$i = 0
while ((Get-Date) -lt $deadline) {
  $i++
  $bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $gfx = [System.Drawing.Graphics]::FromImage($bmp)
  $gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
  $gfx.Dispose()
  $path = Join-Path $dir ("{0:d3}-{1}.png" -f $i, (Get-Date -Format 'HHmmss'))
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output "  $path"
  Start-Sleep -Seconds $Every
}

Write-Output "done: $i frames in $dir"