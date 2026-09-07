# PowerShell script to publish all RIM workspace crates to crates.io in topological order
param(
    [switch]$Execute,
    [switch]$DryRun,
    [int]$DelaySeconds = 25,
    [string]$StartFrom = ""
)

$ErrorActionPreference = "Continue"
if (Test-Path Variable:\PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RimRoot = Split-Path -Parent $ScriptDir

$Crates = @(
    "rimio",
    "rimpart",
    "rimimg",
    "rimfs-core",
    "rimfs-fat",
    "rimfs-exfat",
    "rimfs-ext",
    "rimfs-ntfs",
    "rimfs-tar",
    "rimfs-zip",
    "rimfs-iso",
    "rimfs",
    "rimgen",
    "rimhost",
    "rimcli"
)

Write-Host "=================================================" -ForegroundColor Cyan
Write-Host "       RIM Crates.io Automated Publisher         " -ForegroundColor Cyan
Write-Host "=================================================" -ForegroundColor Cyan
Write-Host "Working Directory : $RimRoot"
Write-Host "Total Crates      : $($Crates.Count)"
Write-Host "Mode              : $(if ($Execute) { 'LIVE PUBLISH (--execute)' } else { 'DRY RUN (preview)' })"
Write-Host "Index Delay       : ${DelaySeconds}s between crates"
if ($StartFrom) {
    Write-Host "Starting from     : $StartFrom"
}
Write-Host "=================================================`n" -ForegroundColor Cyan

if (-not $Execute -and -not $DryRun) {
    Write-Host "Notice: Running in DRY-RUN mode by default." -ForegroundColor Yellow
    Write-Host "Pass -Execute to publish live to crates.io.`n" -ForegroundColor Yellow
}

$Started = if ($StartFrom) { $false } else { $true }
$SuccessList = @()
$SkippedList = @()
$FailedList = @()

Push-Location $RimRoot
try {
    for ($i = 0; $i -lt $Crates.Count; $i++) {
        $crate = $Crates[$i]
        $step = $i + 1
        $prefix = "[$step/$($Crates.Count)]"

        if (-not $Started) {
            if ($crate -eq $StartFrom) {
                $Started = $true
            } else {
                Write-Host "$prefix Skipping $crate (waiting for $StartFrom)..." -ForegroundColor DarkGray
                $SkippedList += $crate
                continue
            }
        }

        Write-Host "$prefix Processing '$crate'..." -ForegroundColor Cyan

        if ($Execute) {
            Write-Host "  -> Running: cargo publish -p $crate" -ForegroundColor Yellow
            $output = cmd.exe /c "cargo publish -p $crate 2>&1"
            $exitCode = $LASTEXITCODE
            $outputStr = ($output -join "`n")
            Write-Host $outputStr

            if ($exitCode -ne 0) {
                if ($outputStr -match "already uploaded" -or $outputStr -match "already exists") {
                    Write-Host "  [OK] Crate $crate is already published at this version." -ForegroundColor Green
                    $SuccessList += "$crate (already published)"
                } else {
                    Write-Host "  [ERROR] Failed to publish $crate!" -ForegroundColor Red
                    $FailedList += $crate
                    throw "Publication failed at crate $crate"
                }
            } else {
                Write-Host "  [OK] Successfully published $crate to crates.io!" -ForegroundColor Green
                $SuccessList += $crate
            }

            # If not the last crate, wait for crates.io index to update
            if ($i -lt ($Crates.Count - 1)) {
                Write-Host "  Waiting ${DelaySeconds}s for crates.io index propagation..." -ForegroundColor DarkYellow
                Start-Sleep -Seconds $DelaySeconds
            }
        } else {
            Write-Host "  -> [Dry-Run] cargo publish -p $crate --dry-run" -ForegroundColor Gray
            $output = cmd.exe /c "cargo publish -p $crate --dry-run 2>&1"
            $exitCode = $LASTEXITCODE
            $outputStr = ($output -join "`n")
            if ($exitCode -eq 0) {
                Write-Host "  [OK] Dry-run succeeded for $crate" -ForegroundColor Green
                $SuccessList += $crate
            } else {
                Write-Host "  [NOTE] Dry-run for dependent crate $crate requires prior dependencies to be published." -ForegroundColor DarkYellow
                $SkippedList += $crate
            }
        }
        Write-Host ""
    }
} finally {
    Pop-Location
}

Write-Host "`n=================================================" -ForegroundColor Cyan
Write-Host "                 SUMMARY REPORT                  " -ForegroundColor Cyan
Write-Host "=================================================" -ForegroundColor Cyan
Write-Host "Successful / Ready : $($SuccessList.Count)" -ForegroundColor Green
foreach ($s in $SuccessList) { Write-Host "  + $s" -ForegroundColor Green }
if ($SkippedList.Count -gt 0) {
    Write-Host "Skipped / Pending  : $($SkippedList.Count)" -ForegroundColor DarkYellow
    foreach ($sk in $SkippedList) { Write-Host "  ~ $sk" -ForegroundColor DarkYellow }
}
if ($FailedList.Count -gt 0) {
    Write-Host "Failed             : $($FailedList.Count)" -ForegroundColor Red
    foreach ($f in $FailedList) { Write-Host "  ! $f" -ForegroundColor Red }
}
Write-Host "=================================================" -ForegroundColor Cyan
