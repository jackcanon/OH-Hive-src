<#
    Windows first-run smoke test for the Den's Tauri installer.

    Written 2026-09-16 (Loki) for the pre-Friday Windows/Linux bring-up. The point is to answer
    "what happens when one of our users runs this" with evidence rather than impressions, and to
    leave a transcript we can diff against the next build.

    It does NOT test silently-and-hope: every step records what it found, and a failure in one
    step does not stop the rest, because the interesting output is the whole picture.

    Usage (from an elevated PowerShell on the Windows machine):
        powershell -ExecutionPolicy Bypass -File windows-smoke-test.ps1 -Installer C:\path\to\Hive_0.3.0_x64_en-US.msi

    Leaves: a log directory (default C:\den-smoke) with the MSI log, the installed file tree,
    the Add/Remove Programs entry as a user would see it, and any launch errors.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [string]$LogDir = 'C:\den-smoke',
    # Skip the install and only re-run the inspection steps, for iterating after a manual install.
    [switch]$InspectOnly
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$transcript = Join-Path $LogDir 'smoke-test.txt'

function Section($name) {
    $line = "`n===== $name =====`n"
    Write-Host $line -ForegroundColor Cyan
    Add-Content -Path $transcript -Value $line
}
function Record($text) {
    Write-Host $text
    Add-Content -Path $transcript -Value $text
}

Set-Content -Path $transcript -Value "Den Windows smoke test - $(Get-Date -Format o)"

Section 'Machine'
Record "OS            : $((Get-CimInstance Win32_OperatingSystem).Caption)"
Record "Version       : $([System.Environment]::OSVersion.Version)"
Record "Arch          : $env:PROCESSOR_ARCHITECTURE"
Record "User          : $env:USERNAME"
# WebView2 is what the Tauri shell actually renders in. Its absence is the single most common
# reason a Tauri app installs fine and then shows a blank or dead window.
$wv = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue
Record "WebView2      : $(if ($wv) { $wv.pv } else { 'NOT FOUND - the app window will likely be blank' })"

Section 'Installer file, as the user received it'
$item = Get-Item -LiteralPath $Installer
Record "Name          : $($item.Name)"
Record "Size          : $([math]::Round($item.Length/1MB,1)) MB"
Record "SHA256        : $((Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash)"
# An unsigned installer is expected right now, but record it rather than assume: this is exactly
# what drives SmartScreen and the "Unknown publisher" UAC prompt a real user sees.
$sig = Get-AuthenticodeSignature -LiteralPath $Installer
Record "Signature     : $($sig.Status)"
Record "Signer        : $(if ($sig.SignerCertificate) { $sig.SignerCertificate.Subject } else { '(unsigned)' })"
# Mark-of-the-web: present when the file came from a browser download, absent when copied via SSH.
# A real user WILL have it, so note when our test does not reproduce that condition.
$zone = Get-Content -LiteralPath $Installer -Stream Zone.Identifier -ErrorAction SilentlyContinue
Record "Zone.Identifier: $(if ($zone) { 'present (browser-downloaded, SmartScreen applies)' } else { 'absent (copied, not downloaded - a real user will see MORE friction than this run does)' })"

if (-not $InspectOnly) {
    Section 'Install'
    if ($Installer -match '\.msi$') {
        $msiLog = Join-Path $LogDir 'msi-install.log'
        Record "msiexec /i ... /qn with verbose logging -> $msiLog"
        $p = Start-Process msiexec.exe -ArgumentList @('/i', "`"$Installer`"", '/qn', '/l*v', "`"$msiLog`"") -Wait -PassThru
        Record "Exit code     : $($p.ExitCode)  $(if ($p.ExitCode -eq 0) { '(success)' } elseif ($p.ExitCode -eq 1603) { '(fatal error during installation)' } elseif ($p.ExitCode -eq 1618) { '(another install in progress)' } else { '' })"
        if (Test-Path $msiLog) {
            Record "`n-- MSI log, lines that matter --"
            Select-String -Path $msiLog -Pattern 'Error|Failed|returned 3|Note: 1:' | Select-Object -First 25 | ForEach-Object { Record "  $($_.Line.Trim())" }
        }
    } else {
        Record 'NSIS installer: /S for silent'
        $p = Start-Process $Installer -ArgumentList '/S' -Wait -PassThru
        Record "Exit code     : $($p.ExitCode)"
    }
}

Section 'What the user sees in Apps & Features'
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$found = Get-ItemProperty $paths -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -match 'Hive|Loki|Den' }
if ($found) {
    $found | ForEach-Object {
        Record "DisplayName   : $($_.DisplayName)"
        Record "DisplayVersion: $($_.DisplayVersion)"
        Record "Publisher     : $(if ($_.Publisher) { $_.Publisher } else { '(blank - shows as unknown)' })"
        Record "InstallLocation: $($_.InstallLocation)"
        Record "URLInfoAbout  : $(if ($_.URLInfoAbout) { $_.URLInfoAbout } else { '(none - no support link)' })"
        Record ''
    }
} else {
    Record 'NOT FOUND - nothing matching Hive/Loki/Den is registered as installed.'
}

Section 'Installed files'
$candidates = @(
    "$env:ProgramFiles\Hive",
    "${env:ProgramFiles(x86)}\Hive",
    "$env:LOCALAPPDATA\Hive",
    "$env:ProgramFiles\Loki's Den"
) | Where-Object { Test-Path $_ }
if ($candidates) {
    foreach ($c in $candidates) {
        Record "-- $c"
        Get-ChildItem -Recurse -File $c -ErrorAction SilentlyContinue |
            Select-Object -First 40 |
            ForEach-Object { Record ("   {0,8:N0} KB  {1}" -f ($_.Length/1KB), $_.FullName.Substring($c.Length+1)) }
    }
} else {
    Record 'No install directory found in the usual locations.'
}

Section 'Shortcuts'
@("$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
  "$env:APPDATA\Microsoft\Windows\Start Menu\Programs",
  "$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop") | ForEach-Object {
    if (Test-Path $_) {
        Get-ChildItem -Path $_ -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match 'Hive|Loki|Den' } |
            ForEach-Object { Record "  $($_.FullName)" }
    }
}

Section 'Launch'
$exe = Get-ChildItem -Recurse -Filter '*.exe' -Path $candidates -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notmatch 'uninstall|webview' } | Select-Object -First 1
if ($exe) {
    Record "Launching: $($exe.FullName)"
    $proc = Start-Process $exe.FullName -PassThru
    Start-Sleep -Seconds 12
    $alive = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
    if ($alive) {
        Record "STILL RUNNING after 12s (pid $($proc.Id)) - it did not crash on startup."
        Record "Window title  : '$($alive.MainWindowTitle)'"
        Record "Private memory: $([math]::Round($alive.PrivateMemorySize64/1MB,1)) MB"
        # A Tauri window that fails to create a webview often survives as a process with no window.
        if (-not $alive.MainWindowTitle) {
            Record 'WARNING: process alive but no window title - a webview that failed to initialise looks exactly like this.'
        }
    } else {
        Record "EXITED within 12s - it crashed or closed itself. Check the Application event log below."
    }
} else {
    Record 'No launchable .exe found.'
}

Section 'Application event log, last 10 minutes'
Get-WinEvent -FilterHashtable @{ LogName='Application'; StartTime=(Get-Date).AddMinutes(-10) } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'Hive|hive|WebView|\.NET|Tauri' } |
    Select-Object -First 15 |
    ForEach-Object { Record "  [$($_.LevelDisplayName)] $($_.TimeCreated): $(($_.Message -split "`n")[0])" }

Section 'Done'
Record "Full transcript: $transcript"
if (Test-Path (Join-Path $LogDir 'msi-install.log')) { Record "MSI log:        $(Join-Path $LogDir 'msi-install.log')" }
