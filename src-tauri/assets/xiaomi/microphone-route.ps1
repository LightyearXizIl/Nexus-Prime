[CmdletBinding()]
param(
  [ValidateSet("EnsureCable", "Restore")]
  [string] $Action = "EnsureCable",
  [string] $PreviousId = "",
  [switch] $Server
)

$ErrorActionPreference = "Stop"

if ($Server) {
  [Console]::InputEncoding = [System.Text.Encoding]::UTF8
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
}

function Emit-Result([bool] $Ok, [string] $Message, [bool] $Changed = $false, [bool] $Skipped = $false, [string] $CurrentId = "", [string] $TargetId = "", [string] $Previous = "") {
  [pscustomobject]@{
    ok = $Ok
    changed = $Changed
    skipped = $Skipped
    current_id = $CurrentId
    target_id = $TargetId
    previous_id = $Previous
    message = $Message
  } | ConvertTo-Json -Compress
}

function Get-VBCableCapture {
  $root = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture"
  foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
    $state = (Get-ItemProperty -LiteralPath $key.PSPath -Name DeviceState -ErrorAction SilentlyContinue).DeviceState
    if ($null -ne $state -and [int]$state -ne 1) { continue }
    $props = Get-ItemProperty -LiteralPath (Join-Path $key.PSPath "Properties") -ErrorAction SilentlyContinue
    $name = "$($props.'{a45c254e-df1c-4efd-8020-67d146a850e0},2') $($props.'{b3f8fa53-0004-438e-9003-51a46e139bfc},6')".Trim()
    if ($name -match "(?i)(^|\s)CABLE Output(\s|$)") {
      return [pscustomobject]@{ Id = "{0.0.1.00000000}.$($key.PSChildName)"; Name = $name }
    }
  }
  return $null
}

function Initialize-AudioEndpointApi {
  if ("NexusMicrophoneRoute" -as [type]) { return }
  Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public enum NexusDataFlow { eRender = 0, eCapture = 1, eAll = 2 }
public enum NexusRole { eConsole = 0, eMultimedia = 1, eCommunications = 2 }

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
internal class NexusMMDeviceEnumeratorComObject {}

[ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface NexusIMMDeviceEnumerator {
  int EnumAudioEndpoints(NexusDataFlow flow, uint mask, out IntPtr devices);
  int GetDefaultAudioEndpoint(NexusDataFlow flow, NexusRole role, out NexusIMMDevice device);
  int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out NexusIMMDevice device);
  int RegisterEndpointNotificationCallback(IntPtr client);
  int UnregisterEndpointNotificationCallback(IntPtr client);
}

[ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface NexusIMMDevice {
  int Activate(ref Guid iid, uint context, IntPtr args, out IntPtr instance);
  int OpenPropertyStore(uint access, out IntPtr properties);
  int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
  int GetState(out uint state);
}

[ComImport, Guid("870AF99C-171D-4F9E-AF0D-E63DF40C2BC9"), ClassInterface(ClassInterfaceType.None)]
internal class NexusPolicyConfigClient {}

[ComImport, Guid("F8679F50-850A-41CF-9C72-430F290290C8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface NexusIPolicyConfig {
  int GetMixFormat(string d, out IntPtr f); int GetDeviceFormat(string d, int x, out IntPtr f);
  int ResetDeviceFormat(string d); int SetDeviceFormat(string d, IntPtr a, IntPtr b);
  int GetProcessingPeriod(string d, int x, out long a, out long b); int SetProcessingPeriod(string d, ref long p);
  int GetShareMode(string d, IntPtr m); int SetShareMode(string d, IntPtr m);
  int GetPropertyValue(string d, IntPtr k, IntPtr v); int SetPropertyValue(string d, IntPtr k, IntPtr v);
  int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string d, NexusRole role); int SetEndpointVisibility(string d, int v);
}

public static class NexusMicrophoneRoute {
  public static string GetDefaultCapture() {
    NexusIMMDeviceEnumerator e = (NexusIMMDeviceEnumerator)new NexusMMDeviceEnumeratorComObject();
    NexusIMMDevice d = null;
    try {
      int hr = e.GetDefaultAudioEndpoint(NexusDataFlow.eCapture, NexusRole.eConsole, out d);
      if (hr != 0) Marshal.ThrowExceptionForHR(hr);
      string id; hr = d.GetId(out id);
      if (hr != 0) Marshal.ThrowExceptionForHR(hr);
      return id;
    } finally {
      if (d != null) Marshal.ReleaseComObject(d);
      Marshal.ReleaseComObject(e);
    }
  }

  public static bool CaptureEndpointExists(string id) {
    NexusIMMDeviceEnumerator e = (NexusIMMDeviceEnumerator)new NexusMMDeviceEnumeratorComObject();
    NexusIMMDevice d = null;
    try {
      int hr = e.GetDevice(id, out d);
      if (hr != 0 || d == null) return false;
      uint state; hr = d.GetState(out state);
      return hr == 0 && state == 1;
    } catch {
      return false;
    } finally {
      if (d != null) Marshal.ReleaseComObject(d);
      Marshal.ReleaseComObject(e);
    }
  }

  public static void SetDefaultCapture(string id) {
    NexusIPolicyConfig c = (NexusIPolicyConfig)new NexusPolicyConfigClient();
    try {
      for (int r = 0; r < 3; r++) {
        int hr = c.SetDefaultEndpoint(id, (NexusRole)r);
        if (hr != 0) Marshal.ThrowExceptionForHR(hr);
      }
    } finally {
      Marshal.ReleaseComObject(c);
    }
  }
}
'@
}

function Invoke-Route([string] $RequestedAction, [string] $RequestedPreviousId) {
  try {
  $cable = Get-VBCableCapture
  if (-not $cable) {
    return Emit-Result $false "CABLE Output is not available"
  }

  Initialize-AudioEndpointApi
  $current = [NexusMicrophoneRoute]::GetDefaultCapture()

  if ($RequestedAction -eq "EnsureCable") {
    if ($current -eq $cable.Id) {
      return Emit-Result $true "CABLE Output is already the default capture device" $false $false $current $cable.Id ""
    }
    [NexusMicrophoneRoute]::SetDefaultCapture($cable.Id)
    $after = [NexusMicrophoneRoute]::GetDefaultCapture()
    if ($after -ne $cable.Id) { throw "CABLE Output did not become the default capture device" }
    return Emit-Result $true "Default capture device changed to CABLE Output" $true $false $current $cable.Id $current
  }

  if ($current -ne $cable.Id) {
    return Emit-Result $true "User selected another capture device; restore skipped" $false $true $current $cable.Id $RequestedPreviousId
  }
  if ([string]::IsNullOrWhiteSpace($RequestedPreviousId)) {
    return Emit-Result $true "No previous capture device was recorded; restore skipped" $false $true $current $cable.Id ""
  }
  if (-not [NexusMicrophoneRoute]::CaptureEndpointExists($RequestedPreviousId)) {
    return Emit-Result $true "Previous capture device is no longer available; restore skipped" $false $true $current $cable.Id $RequestedPreviousId
  }
  [NexusMicrophoneRoute]::SetDefaultCapture($RequestedPreviousId)
  $after = [NexusMicrophoneRoute]::GetDefaultCapture()
  if ($after -ne $RequestedPreviousId) { throw "Previous capture device did not become the default capture device" }
  return Emit-Result $true "Previous capture device restored" $true $false $after $cable.Id $RequestedPreviousId
  } catch {
    return Emit-Result $false $_.Exception.Message
  }
}

if ($Server) {
  while ($null -ne ($line = [Console]::In.ReadLine())) {
    try {
      $request = $line | ConvertFrom-Json
      $requestedAction = [string]$request.action
      $requestedPreviousId = [string]$request.previous_id
      $response = Invoke-Route $requestedAction $requestedPreviousId
    } catch {
      $response = Emit-Result $false $_.Exception.Message
    }
    [Console]::Out.WriteLine($response)
    [Console]::Out.Flush()
  }
  exit 0
}

$response = Invoke-Route $Action $PreviousId
[Console]::Out.WriteLine($response)
try {
  $parsed = $response | ConvertFrom-Json
  if (-not [bool]$parsed.ok) { exit 1 }
  exit 0
} catch {
  exit 1
}
