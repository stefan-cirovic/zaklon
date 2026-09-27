# Runs ON the Windows test machine. Replaces the standalone hub (scheduled
# task) with the installed Zaklon app, in the same folder so it keeps the
# existing data (<Root>\data), and starts it hidden in the tray.
#   powershell -NoProfile -ExecutionPolicy Bypass -File install-app.ps1 -Root C:\Zaklon -Installer C:\Zaklon\Zaklon-setup.exe
param([Parameter(Mandatory = $true)][string]$Root, [Parameter(Mandatory = $true)][string]$Installer)
$ErrorActionPreference = 'Continue'

# 1. Stop and remove the old standalone hub and anything left running.
Unregister-ScheduledTask -TaskName 'Zaklon_Hub' -Confirm:$false -ErrorAction SilentlyContinue
Stop-Process -Name 'zaklon-hub', 'zaklon-app' -Force -ErrorAction SilentlyContinue
Get-Process -Name 'kiwix-serve' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# 2. Firewall: allow the app itself, so Windows never asks (a dismissed prompt creates block rules).
$exe = Join-Path $Root 'zaklon-app.exe'
Get-NetFirewallRule | Where-Object { $_.DisplayName -like '*zaklon-app*' -and $_.Action -eq 'Block' } | Remove-NetFirewallRule -ErrorAction SilentlyContinue
Get-NetFirewallRule -DisplayName 'Zaklon app (program*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'Zaklon app (program TCP)' -Direction Inbound -Action Allow -Protocol TCP -Program $exe -Profile Private,Public | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon app (program UDP)' -Direction Inbound -Action Allow -Protocol UDP -Program $exe -Profile Private,Public | Out-Null

# 3. Silent install into the same folder (NSIS: /D must be last and unquoted).
$p = Start-Process -FilePath $Installer -ArgumentList '/S', "/D=$Root" -Wait -PassThru
"installer exit code: $($p.ExitCode)"
if (-not (Test-Path $exe)) { "zaklon-app.exe missing after install"; exit 1 }

# 4. Start it in the logged-on user's session (not in this SSH session), hidden in the tray.
$user = (Get-CimInstance Win32_ComputerSystem).UserName
$action = New-ScheduledTaskAction -Execute $exe -Argument '--minimized' -WorkingDirectory $Root
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive
Register-ScheduledTask -TaskName 'Zaklon_FirstStart' -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName 'Zaklon_FirstStart'
Start-Sleep -Seconds 8
Unregister-ScheduledTask -TaskName 'Zaklon_FirstStart' -Confirm:$false -ErrorAction SilentlyContinue

$app = Get-Process -Name 'zaklon-app' -ErrorAction SilentlyContinue
if ($app) { "app running (pid $($app.Id)) as $user" } else { "app did not start" }
$run = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*zaklon*' } | ForEach-Object { $_.Name + ' = ' + $_.Value }
"start with Windows: " + ($(if ($run) { $run } else { 'not set' }))
"desktop shortcut: " + (Test-Path ([Environment]::GetFolderPath('Desktop') + '\Zaklon.lnk'))
