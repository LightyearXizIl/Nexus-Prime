[CmdletBinding()]
param(
  [ValidateSet('Install', 'InstallElevated', 'Status')][string]$Mode = 'Install',
  [Parameter(Mandatory = $true)][string]$PackageDir,
  [Parameter(Mandatory = $true)][string]$DllSource,
  [switch]$Force
)

$ErrorActionPreference = 'Stop'
$StateRoot = Join-Path $env:LOCALAPPDATA 'com.lightyear.nexusprime\winuhid'
$RebootFlag = Join-Path $StateRoot 'reboot-required.flag'
$InstallLog = Join-Path $StateRoot 'install.log'
$HardwareId = 'Root\WinUHid'
$DeviceDescription = 'WinUHid Virtual HID Enumerator'
$script:RestartRequired = $false

function Write-Phase([string]$Name, [string]$Detail) {
  # Console output is visible to the app log but does not become a PowerShell
  # pipeline value when callers capture a function's real return value.
  $line = ('[{0:yyyy-MM-dd HH:mm:ss}] Phase: {1} | {2}' -f (Get-Date), $Name, $Detail)
  [Console]::Out.WriteLine($line)
  try {
    New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null
    Add-Content -LiteralPath $InstallLog -Value $line -Encoding UTF8
  } catch {
    # Do not mask the driver-install result merely because diagnostic logging failed.
  }
}

function Test-WinUHidDevice {
  try {
    $stream = [IO.File]::Open('\\.\WinUHid', 'Open', 'ReadWrite', 'ReadWrite')
    $stream.Close()
    return $true
  } catch { return $false }
}

function Initialize-RootDeviceInstaller {
  if ('WinUHidRootInstaller' -as [type]) { return }
  Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class WinUHidRootInstaller {
  const uint DICD_GENERATE_ID=0x1, SPDRP_HARDWAREID=0x1, DIF_REGISTERDEVICE=0x19, DIF_INSTALLDEVICE=0x2, SPDIT_COMPATDRIVER=0x2;
  static readonly IntPtr INVALID_HANDLE_VALUE=new IntPtr(-1);
  [StructLayout(LayoutKind.Sequential)] struct SP_DEVINFO_DATA { public uint cbSize; public Guid ClassGuid; public uint DevInst; public IntPtr Reserved; }
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct SP_DRVINFO_DATA { public uint cbSize; public uint DriverType; public IntPtr Reserved; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=256)] public string Description; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=256)] public string MfgName; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=256)] public string ProviderName; public long DriverDate; public long DriverVersion; }
  [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid classGuid,IntPtr hwndParent);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiCreateDeviceInfo(IntPtr set,string name,ref Guid guid,string desc,IntPtr hwnd,uint flags,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiOpenDeviceInfo(IntPtr set,string instanceId,IntPtr hwnd,uint flags,ref SP_DEVINFO_DATA data);
  // SPDRP_HARDWAREID is a UTF-16 MULTI_SZ value. Explicit Unicode is essential:
  // the ANSI entry point splits the UTF-16 buffer into one hardware ID per letter.
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr set,ref SP_DEVINFO_DATA data,uint property,byte[] buffer,uint size);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiBuildDriverInfoList(IntPtr set,ref SP_DEVINFO_DATA data,uint driverType);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiEnumDriverInfo(IntPtr set,ref SP_DEVINFO_DATA data,uint driverType,uint index,ref SP_DRVINFO_DATA info);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiSetSelectedDriver(IntPtr set,ref SP_DEVINFO_DATA data,ref SP_DRVINFO_DATA info);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint installFunction,IntPtr set,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
  static void Check(bool ok,string operation){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error(),operation);}
  public static void RegisterRootDevice(string hardwareId,string description) {
    Guid systemClass=new Guid("4d36e97d-e325-11ce-bfc1-08002be10318");
    IntPtr set=SetupDiCreateDeviceInfoList(ref systemClass,IntPtr.Zero);
    if(set==INVALID_HANDLE_VALUE)throw new Win32Exception(Marshal.GetLastWin32Error());
    try {
      SP_DEVINFO_DATA data=new SP_DEVINFO_DATA(); data.cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      Check(SetupDiCreateDeviceInfo(set,description,ref systemClass,description,IntPtr.Zero,DICD_GENERATE_ID,ref data),"SetupDiCreateDeviceInfo");
      byte[] ids=Encoding.Unicode.GetBytes(hardwareId+"\0\0");
      Check(SetupDiSetDeviceRegistryProperty(set,ref data,SPDRP_HARDWAREID,ids,(uint)ids.Length),"SetupDiSetDeviceRegistryProperty");
      Check(SetupDiCallClassInstaller(DIF_REGISTERDEVICE,set,ref data),"SetupDiCallClassInstaller(DIF_REGISTERDEVICE)");
    } finally { SetupDiDestroyDeviceInfoList(set); }
  }
  public static void InstallMatchingDriver(string instanceId) {
    Guid systemClass=new Guid("4d36e97d-e325-11ce-bfc1-08002be10318");
    IntPtr set=SetupDiCreateDeviceInfoList(ref systemClass,IntPtr.Zero);
    if(set==INVALID_HANDLE_VALUE)throw new Win32Exception(Marshal.GetLastWin32Error());
    try {
      SP_DEVINFO_DATA data=new SP_DEVINFO_DATA(); data.cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      Check(SetupDiOpenDeviceInfo(set,instanceId,IntPtr.Zero,0,ref data),"SetupDiOpenDeviceInfo");
      Check(SetupDiBuildDriverInfoList(set,ref data,SPDIT_COMPATDRIVER),"SetupDiBuildDriverInfoList");
      SP_DRVINFO_DATA driver=new SP_DRVINFO_DATA(); driver.cbSize=(uint)Marshal.SizeOf(typeof(SP_DRVINFO_DATA));
      Check(SetupDiEnumDriverInfo(set,ref data,SPDIT_COMPATDRIVER,0,ref driver),"SetupDiEnumDriverInfo");
      Check(SetupDiSetSelectedDriver(set,ref data,ref driver),"SetupDiSetSelectedDriver");
      Check(SetupDiCallClassInstaller(DIF_INSTALLDEVICE,set,ref data),"SetupDiCallClassInstaller(DIF_INSTALLDEVICE)");
    } finally { SetupDiDestroyDeviceInfoList(set); }
  }
}
'@
}

function Get-RootDeviceInstanceId {
  # Root-device instance IDs derive from the description. Check HardwareID in
  # the PnP registry instead of mistaking Root\WinUHid for an instance ID.
  $root = 'HKLM:\SYSTEM\CurrentControlSet\Enum\ROOT\WINUHID_VIRTUAL_HID_ENUMERATOR'
  foreach ($node in @(Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {
    $ids = @((Get-ItemProperty -LiteralPath $node.PSPath -Name HardwareID -ErrorAction SilentlyContinue).HardwareID)
    if ($ids | Where-Object { $_ -eq $HardwareId }) { return "ROOT\WINUHID_VIRTUAL_HID_ENUMERATOR\$($node.PSChildName)" }
  }
  return $null
}

function Get-MalformedRootDeviceInstanceIds {
  # Earlier packages called the ANSI SetupAPI entry point with UTF-16 bytes.
  # Those nodes contain one hardware-ID string per character (R, o, o, ...),
  # and can never match the driver. Restrict cleanup to that exact malformed
  # signature so unrelated root-enumerated devices are untouched.
  $root = 'HKLM:\SYSTEM\CurrentControlSet\Enum\ROOT\WINUHID_VIRTUAL_HID_ENUMERATOR'
  foreach ($node in @(Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {
    $ids = @((Get-ItemProperty -LiteralPath $node.PSPath -Name HardwareID -ErrorAction SilentlyContinue).HardwareID)
    if ($ids.Count -gt 1 -and (($ids -join '') -eq $HardwareId)) {
      "ROOT\WINUHID_VIRTUAL_HID_ENUMERATOR\$($node.PSChildName)"
    }
  }
}

function Remove-MalformedRootDeviceNodes([string]$PnputilPath) {
  foreach ($instanceId in @(Get-MalformedRootDeviceInstanceIds)) {
    Invoke-PnputilPhase -PnputilPath $PnputilPath -PhaseName 'CleanMalformedRoot' -Arguments @('/remove-device', $instanceId) -AllowedExitCodes @(0, 259)
  }
}

function Invoke-PnputilPhase {
  param(
    [Parameter(Mandatory = $true)][string]$PnputilPath,
    [Parameter(Mandatory = $true)][string]$PhaseName,
    [Parameter(Mandatory = $true)][string[]]$Arguments,
    [int[]]$AllowedExitCodes = @(0)
  )
  $argLine = $Arguments -join ' '
  Write-Phase $PhaseName "running pnputil $argLine"
  $output = & $PnputilPath @Arguments 2>&1 | Out-String
  if (-not [string]::IsNullOrWhiteSpace($output)) {
    Write-Phase $PhaseName ($output.Trim() -replace '[\r\n]+', ' | ')
  }
  $code = $LASTEXITCODE
  if ($AllowedExitCodes -contains $code) {
    Write-Phase $PhaseName "exit=$code OK"
    if ($code -eq 3010) { $script:RestartRequired = $true }
    return
  }
  throw "pnputil $argLine failed with exit code $code. $($output.Trim())"
}

function Register-RootDeviceNode {
  $existing = Get-RootDeviceInstanceId
  if ($existing) {
    Write-Phase 'RegisterRoot' "node already exists ($existing)"
    return $existing
  }
  Initialize-RootDeviceInstaller
  [WinUHidRootInstaller]::RegisterRootDevice($HardwareId, $DeviceDescription)
  $created = Get-RootDeviceInstanceId
  if (-not $created) { throw 'SetupAPI registered the root device but Windows did not expose its instance ID' }
  Write-Phase 'RegisterRoot' "SetupAPI DIF_REGISTERDEVICE OK ($created)"
  return $created
}

function Bind-AndPresentRootDevice([string]$InfPath, [string]$PnputilPath) {
  Initialize-RootDeviceInstaller
  $instanceId = Register-RootDeviceNode
  Write-Phase 'BindDriver' "selecting staged compatible driver for $instanceId"
  [WinUHidRootInstaller]::InstallMatchingDriver($instanceId)
  Write-Phase 'BindDriver' 'SetupAPI DIF_INSTALLDEVICE OK'
  Invoke-PnputilPhase -PnputilPath $PnputilPath -PhaseName 'ScanDevices' -Arguments @('/scan-devices') -AllowedExitCodes @(0) | Out-Null
}

function Wait-WinUHidReady {
  for ($i = 0; $i -lt 12; $i++) {
    if (Test-WinUHidDevice) { return $true }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

function Install-PublisherCert([string]$Path) {
  $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::new($Path)
  foreach ($storeName in @('Root', 'TrustedPublisher')) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new($storeName, 'LocalMachine')
    $store.Open('ReadWrite')
    try {
      if (-not ($store.Certificates | Where-Object { $_.Thumbprint -eq $cert.Thumbprint })) { $store.Add($cert) }
    } finally { $store.Close() }
  }
}

function Deploy-UserDll {
  if (-not (Test-Path -LiteralPath $DllSource)) { throw "Missing WinUHid SDK DLL: $DllSource" }
  $target = Join-Path $StateRoot 'WinUHid.dll'
  New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null
  Copy-Item -LiteralPath $DllSource -Destination $target -Force
}

function Invoke-ElevatedInstall {
  $forceArgument = if ($Force) { ' -Force' } else { '' }
  $arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Mode InstallElevated -PackageDir "{1}" -DllSource "{2}"{3}' -f $PSCommandPath, $PackageDir, $DllSource, $forceArgument
  try { $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -PassThru -Wait }
  catch { throw "WinUHid driver installation was cancelled or could not elevate: $($_.Exception.Message)" }
  if ($null -eq $process) { throw 'WinUHid driver installation did not start' }
  if ($process.ExitCode -notin @(0, 3010)) {
    $detail = (Get-Content -LiteralPath $InstallLog -Tail 1 -ErrorAction SilentlyContinue)
    if ($detail) { throw "WinUHid driver installation failed with exit code $($process.ExitCode): $detail" }
    throw "WinUHid driver installation failed with exit code $($process.ExitCode)"
  }
  return $process.ExitCode
}

try {
  New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null
  switch ($Mode) {
    'Status' {
      if (Test-WinUHidDevice) { Write-Phase 'Verify' 'device reachable'; Write-Output 'Result: OK'; exit 0 }
      Write-Phase 'Verify' 'device not accessible'
      Write-Output 'Result: WARNING: WinUHid device not accessible'
      exit 1
    }
    'InstallElevated' {
      if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator rights are required' }
      foreach ($name in @('WinUHidDriver.inf', 'WinUHidDriver.dll', 'WinUHidDriver.cat')) {
        if (-not (Test-Path -LiteralPath (Join-Path $PackageDir $name))) { throw "Missing driver package file: $name" }
      }
      $certPath = Join-Path (Split-Path -Parent $PackageDir) 'WinUHidPublisher.cer'
      if (-not (Test-Path -LiteralPath $certPath)) { throw "Missing driver certificate: $certPath" }
      Write-Phase 'Prepare' 'install publisher certificate and deploy DLL'
      Install-PublisherCert $certPath
      Deploy-UserDll
      $pnputil = Join-Path $env:SystemRoot 'System32\pnputil.exe'
      if (-not (Test-Path -LiteralPath $pnputil)) { throw 'pnputil.exe not found' }
      Invoke-PnputilPhase -PnputilPath $pnputil -PhaseName 'StageDriver' -Arguments @('/add-driver', (Join-Path $PackageDir 'WinUHidDriver.inf')) -AllowedExitCodes @(0, 259, 3010) | Out-Null
      Remove-MalformedRootDeviceNodes $pnputil
      Bind-AndPresentRootDevice (Join-Path $PackageDir 'WinUHidDriver.inf') $pnputil
      Write-Phase 'Verify' 'waiting for device'
      if (Wait-WinUHidReady) {
        Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue
        Write-Phase 'Verify' 'device reachable'
        Write-Output 'Result: OK'
        exit 0
      }
      Set-Content -LiteralPath $RebootFlag -Value 'reboot required' -Encoding ASCII
      Write-Phase 'Verify' 'device not reachable after bind and scan; restart required'
      Write-Output 'Result: RESTART_REQUIRED'
      exit 3010
    }
    'Install' {
      if ((Test-WinUHidDevice) -and -not $Force) { Write-Phase 'Verify' 'already reachable'; Write-Output 'Result: OK'; exit 0 }
      $code = Invoke-ElevatedInstall
      if (Test-WinUHidDevice) { Write-Phase 'Verify' 'reachable after elevated install'; Write-Output 'Result: OK'; exit 0 }
      if ($code -eq 3010 -or $script:RestartRequired -or (Test-Path -LiteralPath $RebootFlag)) { Write-Phase 'Verify' 'restart required'; Write-Output 'Result: RESTART_REQUIRED'; exit 3010 }
      throw 'WinUHid driver installed but device is still inaccessible'
    }
  }
} catch {
  $message = $_.Exception.GetBaseException().Message
  Write-Phase 'Error' $message
  Write-Output "Result: WARNING: $message"
  exit 1
}
