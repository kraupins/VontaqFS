# VontaqFS / Figma connection observation (read-only, Windows PowerShell 5.1+).
# Use while a Figma PUT is pending. No HTTP payloads, credentials, URL paths, or tokens are collected.
[CmdletBinding()]
param(
    [ValidateRange(1, 65535)][int]$Port = 47833,
    [ValidateRange(10, 600)][int]$Seconds = 90,
    [ValidateRange(250, 5000)][int]$IntervalMs = 1000,
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path (Get-Location).Path 'VFS_FIGMA_PUT_AUDIT'
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$runId = Get-Date -Format 'yyyyMMdd_HHmmss'
$csvPath = Join-Path $OutputDirectory "sockets_$runId.csv"
$healthPath = Join-Path $OutputDirectory "health_$runId.csv"
$summaryPath = Join-Path $OutputDirectory "summary_$runId.txt"

$socketRows = New-Object 'System.Collections.Generic.List[object]'
$healthRows = New-Object 'System.Collections.Generic.List[object]'
$knownPidNames = @{}
$started = Get-Date
$stopAt = $started.AddSeconds($Seconds)
$nextHealth = $started
$nextStatus = $started
$sampleCount = 0

function Resolve-ProcessName([int]$ProcessId) {
    if ($knownPidNames.ContainsKey($ProcessId)) { return $knownPidNames[$ProcessId] }
    $processName = '<exited-or-inaccessible>'
    try {
        $processName = (Get-Process -Id $ProcessId -ErrorAction Stop).ProcessName
    } catch { }
    $knownPidNames[$ProcessId] = $processName
    return $processName
}

Write-Host "Observing port $Port for $Seconds seconds. Keep the Figma PUT pending during this capture."
Write-Host 'Collects TCP states and process names only; health checks use GET, no authorization.'
try {
    while ((Get-Date) -lt $stopAt) {
        $now = Get-Date
        $sampleCount++
        $connections = @(Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object {
            ($_.LocalPort -eq $Port -or $_.RemotePort -eq $Port) -and
            $_.State -notin @('Listen', 'TimeWait')
        })
        foreach ($connection in $connections) {
            $pidValue = [int]$connection.OwningProcess
            $socketRows.Add([PSCustomObject]@{
                timestamp = $now.ToString('o')
                state = [string]$connection.State
                localAddress = [string]$connection.LocalAddress
                localPort = [int]$connection.LocalPort
                remoteAddress = [string]$connection.RemoteAddress
                remotePort = [int]$connection.RemotePort
                pid = $pidValue
                process = Resolve-ProcessName $pidValue
            })
        }
        if ($now -ge $nextHealth) {
            # curl stdout is HTTP status only; stderr is discarded to avoid Windows
            # PowerShell 5.1 treating native diagnostic output as a script failure.
            $httpStatus = ''
            $curlExit = -1
            try {
                $oldErrorActionPreference = $ErrorActionPreference
                $ErrorActionPreference = 'Continue'
                $httpStatus = [string](& curl.exe --silent --noproxy '*' --max-time 3 --output NUL --write-out '%{http_code}' "http://127.0.0.1:$Port/v1/health" 2>$null)
                $curlExit = $LASTEXITCODE
            } catch {
                $httpStatus = ''
            } finally {
                $ErrorActionPreference = $oldErrorActionPreference
            }
            $healthRows.Add([PSCustomObject]@{
                timestamp = (Get-Date).ToString('o')
                status = $httpStatus.Trim()
                exitCode = $curlExit
            })
            $nextHealth = $now.AddSeconds(10)
        }
        if ($now -ge $nextStatus) {
            $figmaCount = @($connections | Where-Object {
                (Resolve-ProcessName ([int]$_.OwningProcess)) -ieq 'Figma'
            }).Count
            Write-Host ('{0} observed active sockets={1}, Figma-owned={2}' -f $now.ToString('HH:mm:ss'), $connections.Count, $figmaCount)
            $nextStatus = $now.AddSeconds(10)
        }
        Start-Sleep -Milliseconds $IntervalMs
    }
} finally {
    if ($socketRows.Count) {
        $socketRows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8
    } else {
        'timestamp,state,localAddress,localPort,remoteAddress,remotePort,pid,process' | Set-Content -LiteralPath $csvPath -Encoding UTF8
    }
    if ($healthRows.Count) {
        $healthRows | Export-Csv -LiteralPath $healthPath -NoTypeInformation -Encoding UTF8
    }
    $observedFigma = @($socketRows | Where-Object { $_.process -ieq 'Figma' })
    $observedVfs = @($socketRows | Where-Object { $_.process -ieq 'vontaqfs-desktop' })
    $failedHealth = @($healthRows | Where-Object { $_.status -ne '200' -or $_.exitCode -ne 0 })
    @(
        'VontaqFS Figma PUT / TCP observation'
        "Started: $($started.ToString('o'))"
        "Duration requested: $Seconds seconds; snapshots: $sampleCount"
        "Port: $Port"
        "Socket rows: $($socketRows.Count)"
        "Figma-owned socket observations: $($observedFigma.Count)"
        "VFS-owned socket observations: $($observedVfs.Count)"
        "Health probes: $($healthRows.Count); non-200 or failed: $($failedHealth.Count)"
        'LIMITATION: port/PID evidence alone cannot associate a TCP connection'
        'with a particular HTTP PUT. It does not prove that request bytes reached VFS.'
        "Sockets file: $csvPath"
        "Health file: $healthPath"
    ) | Set-Content -LiteralPath $summaryPath -Encoding UTF8
    Write-Host ''
    Get-Content -LiteralPath $summaryPath
}
