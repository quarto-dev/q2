<#
  Sends CTRL_C_EVENT to the console of process $TargetPid (GE-17, c1 step 9). A Node parent cannot deliver that event
  to one child, and tokio::signal::ctrl_c() on Windows listens only for it. Exits 0 when the event was generated.
#>
param([Parameter(Mandatory = $true)][int]$TargetPid)
$sig = @'
using System;
using System.Runtime.InteropServices;
public static class K32 {
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint pid);
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GenerateConsoleCtrlEvent(uint ev, uint group);
}
'@
Add-Type -TypeDefinition $sig
[void][K32]::FreeConsole()
if (-not [K32]::AttachConsole([uint32]$TargetPid)) { Write-Error ('AttachConsole failed: ' + [Runtime.InteropServices.Marshal]::GetLastWin32Error()); exit 2 }
[void][K32]::SetConsoleCtrlHandler([IntPtr]::Zero, $true)   # this helper ignores the event it sends
$ok = [K32]::GenerateConsoleCtrlEvent(0, 0)
Start-Sleep -Milliseconds 300
[void][K32]::FreeConsole()
if ($ok) { exit 0 } else { Write-Error 'GenerateConsoleCtrlEvent failed'; exit 3 }
