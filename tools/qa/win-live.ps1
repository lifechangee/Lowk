param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\debug\lowk.exe'),
    [string]$Out = (Join-Path (Join-Path $PSScriptRoot '..\..\target\qa-win-live') (Get-Date -Format 'yyyyMMdd-HHmmss')),
    [switch]$Build,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$script:StartedProcess = $null
$script:DataDir = $null
$script:QaUri = $null
$script:QaToken = $null
$script:Pressed = @{ Ctrl = $false; Alt = $false; Backslash = $false }
$script:Results = @()
$script:HoldDelayAvailable = Test-Path (Join-Path $PSScriptRoot '..\..\crates\lowk-types\src\settings.rs')
$script:HoldDelayDefault = $null
if ($script:HoldDelayAvailable) {
    $settingsText = Get-Content (Join-Path $PSScriptRoot '..\..\crates\lowk-types\src\settings.rs') -Raw
    $script:HoldDelayAvailable = $settingsText -match '\bhold_delay_ms\b'
    if ($settingsText -match '(?m)^\s*hold_delay_ms:\s*(\d+),') {
        $parsedDelay = 0.0
        if ([double]::TryParse($Matches[1], [ref]$parsedDelay)) { $script:HoldDelayDefault = $parsedDelay }
    }
}
$script:HoldDelayReason = 'hold_delay_ms does not exist in crates/lowk-types/src/settings.rs in this checkout.'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LowkWinLiveKeys {
    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT {
        public UInt32 type;
        public INPUTUNION data;
    }
    [StructLayout(LayoutKind.Explicit, Size = 24)]
    private struct INPUTUNION {
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public MOUSEINPUT mi;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct KEYBDINPUT {
        public UInt16 wVk;
        public UInt16 wScan;
        public UInt32 dwFlags;
        public UInt32 time;
        public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct MOUSEINPUT {
        public Int32 dx;
        public Int32 dy;
        public UInt32 mouseData;
        public UInt32 dwFlags;
        public UInt32 time;
        public IntPtr dwExtraInfo;
    }
    [DllImport("user32.dll", SetLastError = true)]
    private static extern UInt32 SendInput(UInt32 nInputs, INPUT[] pInputs, Int32 cbSize);
    public static void Key(UInt16 vk, UInt16 scan, bool up) {
        INPUT i = new INPUT();
        i.type = 1;
        i.data.ki.wVk = vk;
        i.data.ki.wScan = scan;
        i.data.ki.dwFlags = up ? 2u : 0u;
        INPUT[] a = new INPUT[] { i };
        if (SendInput(1, a, Marshal.SizeOf(typeof(INPUT))) != 1)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    }
}
'@

function Get-LogPath {
    return Join-Path $script:DataDir 'logs\lowk.log'
}

function Get-LogLines {
    $p = Get-LogPath
    if (Test-Path $p) { return @(Get-Content -LiteralPath $p) }
    return @()
}

function Get-NewLogLines([int]$Offset) {
    $lines = @(Get-LogLines)
    if ($Offset -ge $lines.Count) { return @() }
    return @($lines | Select-Object -Skip $Offset)
}

function Wait-ForLog([string]$Pattern, [int]$Seconds, [int]$Offset = 0) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
        $found = @(Get-NewLogLines $Offset | Where-Object { $_ -match $Pattern })
        if ($found.Count -gt 0) { return $found }
        Start-Sleep -Milliseconds 100
    }
    return @()
}

function Invoke-Qa([string]$Command) {
    $headers = @{ 'X-Lowk-Qa-Token' = $script:QaToken }
    $response = Invoke-WebRequest -UseBasicParsing -Method Post -Uri ($script:QaUri + '/cmd') `
        -Headers $headers -ContentType 'text/plain' -Body $Command -TimeoutSec 10
    return [string]$response.Content
}

function Get-DesktopScreenshot([string]$Name) {
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size)
        $bitmap.Save((Join-Path $Out ($Name + '.png')), [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Write-Check([string]$Name, [string]$Status, [string]$Reason, [string[]]$Evidence) {
    $entry = '{0}: {1} - {2}' -f $Status, $Name, $Reason
    $script:Results += $entry
    Write-Host $entry
    if ($Evidence -and $Evidence.Count -gt 0) {
        foreach ($line in $Evidence) { Write-Host ('  ' + $line) }
    }
    else {
        Write-Host '  decisive log lines: none'
    }
    Get-DesktopScreenshot ($Name -replace '[^A-Za-z0-9_-]', '-')
}

function Get-LogCount {
    return @(Get-LogLines).Count
}

function Send-Key([string]$Key, [bool]$Up) {
    # Left-hand virtual keys with their scan codes, like a physical keyboard: the hook tracks VK_LCONTROL /
    # VK_LMENU, and the generic VK_CONTROL / VK_MENU of a bare SendInput reach it as unknown keys.
    $vk = 0; $scan = 0
    switch ($Key) {
        'Ctrl' { $vk = 0xA2; $scan = 0x1D }
        'Alt' { $vk = 0xA4; $scan = 0x38 }
        'Backslash' { $vk = 0xDC; $scan = 0x2B }
        default { throw "Unsupported key: $Key" }
    }
    [LowkWinLiveKeys]::Key([UInt16]$vk, [UInt16]$scan, $Up)
    $script:Pressed[$Key] = -not $Up
}

function Start-QaProcess {
    $script:DataDir = Join-Path ([IO.Path]::GetTempPath()) ('lowk-qa-win-live-' + [Guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $script:DataDir -Force)
    $script:QaToken = ([Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N'))
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = [IO.Path]::GetFullPath($Exe)
    $psi.WorkingDirectory = (Get-Location).Path
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $false
    $psi.EnvironmentVariables['LOWK_DATA_DIR'] = $script:DataDir
    $psi.EnvironmentVariables['LOWK_QA_MULTI'] = '1'
    $psi.EnvironmentVariables['LOWK_QA_ACCEPT_INJECTED'] = '1'
    $psi.EnvironmentVariables['LOWK_QA_TOKEN'] = $script:QaToken
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $psi
    if (-not $p.Start()) { throw 'Could not start the QA executable.' }
    $script:StartedProcess = $p
    $qaFile = Join-Path $script:DataDir 'qa.json'
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($p.HasExited) { throw "QA process exited during startup (code $($p.ExitCode))." }
        if (Test-Path $qaFile) {
            $qa = Get-Content -LiteralPath $qaFile -Raw | ConvertFrom-Json
            $script:QaUri = 'http://127.0.0.1:{0}' -f [int]$qa.port
            return
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'Timed out waiting for qa.json.'
}

try {
    [void](New-Item -ItemType Directory -Path $Out -Force)

    $alreadyRunning = @(Get-Process -Name lowk, Lowk -ErrorAction SilentlyContinue)
    if ($alreadyRunning.Count -gt 0) {
        $ids = ($alreadyRunning | ForEach-Object { $_.Id }) -join ', '
        throw "A Lowk process is already running (PID $ids). Quit it yourself before running this check; the script will not stop or access it."
    }

    if ($Build) {
        & pnpm --dir ui build
        if ($LASTEXITCODE -ne 0) { throw "pnpm --dir ui build failed with exit code $LASTEXITCODE." }
        & cargo build -p lowk-app --features 'qa custom-protocol'
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE." }
    }
    if (-not (Test-Path -LiteralPath $Exe)) { throw "QA executable not found: $Exe. Build it or use -Build." }

    Start-QaProcess
    $startupEvidence = @(Wait-ForLog 'QA: the input hook accepts injected input' 20)
    if ($startupEvidence.Count -gt 0) {
        $indicatorEvidence = @(Wait-ForLog 'indicator changed.*to=(Background|Attention)' 20)
        if ($indicatorEvidence.Count -gt 0) {
            $indicatorKind = if ($indicatorEvidence[0] -match 'to=Attention') { 'Attention' } else { 'Background' }
            Write-Check 'startup' 'PASS' "injected-input warning present; indicator became $indicatorKind" ($startupEvidence + $indicatorEvidence)
        }
        else {
            Write-Check 'startup' 'FAIL' 'injected-input warning present, but no Background or Attention indicator transition was logged' $startupEvidence
        }
    }
    else {
        Write-Check 'startup' 'FAIL' 'injected-input warning was not logged' @()
    }

    Write-Host 'Hands off the keyboard. Input will be injected in 5 seconds.'
    for ($n = 5; $n -gt 0; $n--) { Write-Host $n; Start-Sleep -Seconds 1 }

    if ($script:HoldDelayAvailable) {
        if ($null -ne $script:HoldDelayDefault -and $script:HoldDelayDefault -lt 150) {
            Write-Check 'hold-too-short' 'SKIP' 'default delay is too short for a meaningful short-hold check' @()
        }
        else {
            $offset = Get-LogCount
            Send-Key 'Ctrl' $false
            # The default delay is 0.25 s, so 100 ms stays well under it.
            Start-Sleep -Milliseconds 100
            Send-Key 'Ctrl' $true
            Start-Sleep -Milliseconds 1500
            $lines = @(Get-NewLogLines $offset)
            $bad = @($lines | Where-Object { $_ -match 'action=ClickHold phase=Down' })
            if ($bad.Count -eq 0) { Write-Check 'hold-too-short' 'PASS' 'no ClickHold Down within 1.5 s after Ctrl was released' @() }
            else { Write-Check 'hold-too-short' 'FAIL' 'ClickHold Down appeared after a 100 ms hold' $bad }
        }

        $offset = Get-LogCount
        Send-Key 'Ctrl' $false
        Start-Sleep -Milliseconds 1300
        Send-Key 'Ctrl' $true
        $down = @(Wait-ForLog 'action=ClickHold phase=Down' 5 $offset)
        $up = @(Wait-ForLog 'action=ClickHold phase=Up' 5 $offset)
        if ($down.Count -gt 0 -and $up.Count -gt 0) { Write-Check 'hold-long' 'PASS' '1300 ms Ctrl hold logged ClickHold Down and Up' ($down + $up) }
        else { Write-Check 'hold-long' 'FAIL' 'expected ClickHold Down and Up for 1300 ms Ctrl hold' ($down + $up) }
    }
    else {
        Write-Check 'hold-too-short' 'SKIP' $script:HoldDelayReason @()
        Write-Check 'hold-long' 'SKIP' $script:HoldDelayReason @()
    }

    $offset = Get-LogCount
    Send-Key 'Ctrl' $false
    Start-Sleep -Milliseconds 40
    Send-Key 'Backslash' $false
    Start-Sleep -Milliseconds 50
    Send-Key 'Backslash' $true
    Send-Key 'Ctrl' $true
    $toggleDown = @(Wait-ForLog 'action=ToggleOverlay phase=Down' 5 $offset)
    if ($script:HoldDelayAvailable) {
        Start-Sleep -Milliseconds 1500
        $new = @(Get-NewLogLines $offset)
        $clicks = @($new | Where-Object { $_ -match 'action=ClickHold phase=Down' })
        if ($toggleDown.Count -gt 0 -and $clicks.Count -eq 0) {
            $offsetRestore = Get-LogCount
            Send-Key 'Ctrl' $false; Start-Sleep -Milliseconds 40; Send-Key 'Backslash' $false
            Start-Sleep -Milliseconds 50; Send-Key 'Backslash' $true; Send-Key 'Ctrl' $true
            $restored = @(Wait-ForLog 'action=ToggleOverlay phase=Down' 5 $offsetRestore)
            if ($restored.Count -gt 0) { Write-Check 'ctrl-backslash' 'PASS' 'toggle logged and no ClickHold Down; second press restored the overlay' ($toggleDown + $restored) }
            else { Write-Check 'ctrl-backslash' 'FAIL' 'first toggle passed; second press did not log the restore toggle' $toggleDown }
        }
        else { Write-Check 'ctrl-backslash' 'FAIL' 'expected ToggleOverlay Down and no ClickHold Down' ($toggleDown + $clicks) }
    }
    else {
        $offsetRestore = Get-LogCount
        Send-Key 'Ctrl' $false; Start-Sleep -Milliseconds 40; Send-Key 'Backslash' $false
        Start-Sleep -Milliseconds 50; Send-Key 'Backslash' $true; Send-Key 'Ctrl' $true
        $restored = @(Wait-ForLog 'action=ToggleOverlay phase=Down' 5 $offsetRestore)
        if ($toggleDown.Count -gt 0 -and $restored.Count -gt 0) { Write-Check 'ctrl-backslash-toggle' 'PASS' 'both overlay toggles logged' ($toggleDown + $restored) }
        else { Write-Check 'ctrl-backslash-toggle' 'FAIL' 'one or both ToggleOverlay Down events were missing' ($toggleDown + $restored) }
        Write-Check 'ctrl-backslash-clickhold-suppression' 'SKIP' $script:HoldDelayReason @()
    }

    $offset = Get-LogCount
    Send-Key 'Alt' $false
    Start-Sleep -Milliseconds 1300
    Send-Key 'Alt' $true
    $moveDown = @(Wait-ForLog 'action=MoveHold phase=Down' 5 $offset)
    $moveUp = @(Wait-ForLog 'action=MoveHold phase=Up' 5 $offset)
    if ($moveDown.Count -gt 0 -and $moveUp.Count -gt 0) { Write-Check 'alt-hold' 'PASS' '1300 ms Alt hold logged MoveHold Down and Up' ($moveDown + $moveUp) }
    else { Write-Check 'alt-hold' 'FAIL' 'expected MoveHold Down and Up for 1300 ms Alt hold' ($moveDown + $moveUp) }

    $demoStart = Invoke-Qa 'demo-start'
    $script:DemoMayBeRunning = $true
    $hidden = @(Wait-ForLog 'indicator changed.*to=Hidden' 5)
    if ($hidden.Count -gt 0) {
        Write-Check 'demo-start' 'PASS' "demo-start returned '$demoStart'; indicator became Hidden" $hidden
        $stopResponse = Invoke-Qa 'demo-stop'
        $script:DemoMayBeRunning = $false
        $visibleAgain = @(Wait-ForLog 'indicator changed.*to=(Background|Attention)' 10)
        if ($visibleAgain.Count -gt 0) {
            $kind = if ($visibleAgain[-1] -match 'to=Attention') { 'Attention' } else { 'Background' }
            Write-Check 'demo-stop' 'PASS' "demo-stop returned '$stopResponse'; indicator returned to $kind" $visibleAgain
        }
        else { Write-Check 'demo-stop' 'FAIL' "demo-stop returned '$stopResponse'; no Background or Attention transition was logged" @() }
    }
    else {
        Write-Check 'demo-start' 'FAIL' "demo-start returned '$demoStart'; no Hidden indicator transition was logged" @()
        Write-Check 'demo-stop' 'SKIP' 'demo did not reach the Hidden state, so stop-state verification was not run' @()
    }
}
catch {
    $script:FatalError = $true
    Write-Error $_
}
finally {
    foreach ($key in @('Backslash', 'Ctrl', 'Alt')) {
        if ($script:Pressed[$key]) {
            try { Send-Key $key $true } catch { Write-Warning "Could not release $key : $_" }
        }
    }
    if ($script:DemoMayBeRunning -and $script:QaUri -and $script:StartedProcess -and -not $script:StartedProcess.HasExited) {
        try { [void](Invoke-Qa 'demo-stop') } catch { Write-Warning "Could not stop the QA demo session: $_" }
    }
    if ($script:StartedProcess -and -not $script:StartedProcess.HasExited) {
        try { $script:StartedProcess.Kill(); $script:StartedProcess.WaitForExit(5000) | Out-Null } catch { Write-Warning "Could not stop the QA process PID $($script:StartedProcess.Id): $_" }
    }
    if ($script:DataDir -and (Test-Path -LiteralPath $script:DataDir)) {
        if ($Keep) { Write-Host "Kept QA data: $script:DataDir" }
        else { Remove-Item -LiteralPath $script:DataDir -Recurse -Force }
    }
}

$failCount = @($script:Results | Where-Object { $_ -like 'FAIL:*' }).Count
Write-Host "`nResults: $($script:Results.Count - $failCount) non-failing, $failCount failing. Screenshots and output: $([IO.Path]::GetFullPath($Out))"
if ($failCount -gt 0 -or $script:FatalError) { exit 1 }
