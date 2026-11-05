$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot -Parent
$taskStartup = [Environment]::GetFolderPath('Startup')
$taskShell = New-Object -ComObject WScript.Shell
$taskShortcut = $taskShell.CreateShortcut((Join-Path $taskStartup 'GPU Pocket AI Worker.lnk'))
$taskShortcut.TargetPath = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
$taskShortcut.Arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $PSScriptRoot 'start_ai_worker.ps1') + '"'
$taskShortcut.WorkingDirectory = $taskRoot
$taskShortcut.WindowStyle = 7
$taskShortcut.Description = 'GPU Pocket desktop AI task worker'
$taskShortcut.Save()
Write-Output 'GPU Pocket AI worker will start when this Windows user signs in.'
