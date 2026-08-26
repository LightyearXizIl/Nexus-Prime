[CmdletBinding()]
param(
  [ValidateSet('Install', 'InstallElevated')][string]$Mode = 'Install',
  [Parameter(Mandatory = $true)][string]$PackageDir,
  [Parameter(Mandatory = $true)][string]$DllSource,
  [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Test-WinUHidDevice {
  try { $f = [IO.File]::Open('\\.\WinUHid', 'Open', 'ReadWrite', 'ReadWrite'); $f.Close(); return $true } catch { return $false }
}

function Install-Certificate([string]$Path) {
  $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::new($Path)
  foreach ($storeName in @('Root', 'TrustedPublisher')) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new($storeName, 'LocalMachine')
    $store.Open('ReadWrite')
    try {
      if (-not ($store.Certificates | Where-Object { $_.Thumbprint -eq $cert.Thumbprint })) { $store.Add($cert) }
    } finally { $store.Close() }
  }
}

function Install-RootDevice([string]$InfPath) {
  Add-Type -TypeDefinition @'
using System; using System.ComponentModel; using System.Runtime.InteropServices; using System.Text;
public static class WinUHidRootInstaller {
 const uint DICD_GENERATE_ID=1, SPDRP_HARDWAREID=1, DIF_REGISTERDEVICE=0x19, INSTALLFLAG_FORCE=1;
 [StructLayout(LayoutKind.Sequential)] public struct D { public uint cbSize; public Guid ClassGuid; public uint DevInst; public IntPtr Reserved; }
 [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid g,IntPtr h);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiCreateDeviceInfo(IntPtr s,string n,ref Guid g,string d,IntPtr h,uint f,ref D x);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr s,ref D x,uint p,byte[] b,uint z);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint f,IntPtr s,ref D x);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
 [DllImport("newdev.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr h,string id,string inf,uint f,out bool reboot);
 static void C(bool ok){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error());}
 public static bool Install(string inf) { Guid g=new Guid("4d36e97d-e325-11ce-bfc1-08002be10318"); IntPtr s=SetupDiCreateDeviceInfoList(ref g,IntPtr.Zero); if(s==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error()); try { D d=new D(); d.cbSize=(uint)Marshal.SizeOf(typeof(D)); C(SetupDiCreateDeviceInfo(s,"WinUHid Virtual HID Enumerator",ref g,"WinUHid Virtual HID Enumerator",IntPtr.Zero,DICD_GENERATE_ID,ref d)); byte[] ids=Encoding.Unicode.GetBytes("Root\\WinUHid\0\0"); C(SetupDiSetDeviceRegistryProperty(s,ref d,SPDRP_HARDWAREID,ids,(uint)ids.Length)); C(SetupDiCallClassInstaller(DIF_REGISTERDEVICE,s,ref d)); bool reboot; C(UpdateDriverForPlugAndPlayDevices(IntPtr.Zero,"Root\\WinUHid",inf,INSTALLFLAG_FORCE,out reboot)); return reboot; } finally { SetupDiDestroyDeviceInfoList(s); } }
}
'@
  return [WinUHidRootInstaller]::Install([IO.Path]::GetFullPath($InfPath))
}

if ($Mode -eq 'Install') {
  if (-not $Force -and (Test-WinUHidDevice)) { Write-Output 'Result: OK'; exit 0 }
  $forceArg = if ($Force) { ' -Force' } else { '' }
  $args = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Mode InstallElevated -PackageDir `"$PackageDir`" -DllSource `"$DllSource`"$forceArg"
  try {
    $process = Start-Process powershell.exe -ArgumentList $args -Verb RunAs -WindowStyle Hidden -PassThru -Wait
  } catch {
    throw "WinUHid driver installation was cancelled or could not elevate: $($_.Exception.Message)"
  }
  if ($null -eq $process) { throw 'WinUHid driver installation did not start' }
  if ($process.ExitCode -eq 3010) { Write-Output 'Result: RESTART_REQUIRED'; exit 3010 }
  if ($process.ExitCode -eq 1223) { throw 'WinUHid driver installation was cancelled' }
  if ($process.ExitCode -ne 0) { throw "WinUHid driver installation failed with exit code $($process.ExitCode)" }
  if (Test-WinUHidDevice) { Write-Output 'Result: OK'; exit 0 }
  Write-Output 'Result: RESTART_REQUIRED'
  exit 3010
}

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator rights are required' }
foreach ($name in @('WinUHidDriver.inf', 'WinUHidDriver.dll', 'WinUHidDriver.cat')) { if (-not (Test-Path -LiteralPath (Join-Path $PackageDir $name))) { throw "Missing driver package file: $name" } }
$certPath = Join-Path (Split-Path -Parent $PackageDir) 'WinUHidPublisher.cer'
$dllSourcePath = [IO.Path]::GetFullPath($DllSource)
if (-not (Test-Path -LiteralPath $certPath)) { throw "Missing driver certificate: $certPath" }
if (-not (Test-Path -LiteralPath $dllSourcePath)) { throw "Missing WinUHid SDK DLL: $dllSourcePath" }
Install-Certificate $certPath
& "$env:SystemRoot\System32\pnputil.exe" /add-driver (Join-Path $PackageDir 'WinUHidDriver.inf') /install | Out-Null
$pnputilExitCode = $LASTEXITCODE
if ($pnputilExitCode -ne 0 -and $pnputilExitCode -ne 3010) { throw "pnputil failed with exit code $pnputilExitCode" }
$reboot = $false
if (-not (Test-WinUHidDevice)) { $reboot = Install-RootDevice (Join-Path $PackageDir 'WinUHidDriver.inf') }
Start-Sleep -Seconds 3
if ($pnputilExitCode -eq 3010 -or $reboot -or -not (Test-WinUHidDevice)) {
  Write-Output 'Result: RESTART_REQUIRED'
  exit 3010
}
Write-Output 'Result: OK'
