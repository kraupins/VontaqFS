# Read-only Windows provenance check: executable owning a VontaqFS listener.
# Run on the affected host, from a PowerShell process with permission to read the process path.
param([ValidateSet(47833,47834,47835,47836)][int]$Port = 47833)
$ErrorActionPreference = 'Stop'
$listener = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction Stop |
    Where-Object { $_.LocalAddress -eq '127.0.0.1' } | Select-Object -First 1
if ($null -eq $listener) { throw "No IPv4 127.0.0.1 listener found on $Port" }
$pidNumber = [int]$listener.OwningProcess
$processInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $pidNumber"
if ($null -eq $processInfo -or [string]::IsNullOrWhiteSpace($processInfo.ExecutablePath)) {
    throw "Unable to resolve executable of PID $pidNumber"
}
$exe = $processInfo.ExecutablePath
$hash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
$signature = Get-AuthenticodeSignature -LiteralPath $exe
[pscustomobject]@{
    Listener = "127.0.0.1:$Port"
    PID = $pidNumber
    ExecutablePath = $exe
    SHA256 = $hash
    SignatureStatus = [string]$signature.Status
} | Format-List
# Compare this hash to the published installed binary and to a known source-commit build.
# The health.runtimeVersion field alone does not establish source identity.
