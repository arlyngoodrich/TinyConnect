[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        'Speaker (Realtek(R) Audio)',
        'Headset Earphone (CORSAIR HS55 WIRELESS Gaming Headset)'
    )]
    [string] $Name
)

$ErrorActionPreference = 'Stop'
Import-Module AudioDeviceCmdlets -ErrorAction Stop

$device = Get-AudioDevice -List |
    Where-Object { $_.Type -eq 'Playback' -and $_.Name -eq $Name } |
    Select-Object -First 1

if ($null -eq $device) {
    throw "Playback endpoint not found: $Name"
}

Set-AudioDevice -Index $device.Index | Out-Null
$selected = (Get-AudioDevice -Playback).Name

if ($selected -ne $Name) {
    throw "Windows selected '$selected' instead of '$Name'"
}

Write-Output $selected
