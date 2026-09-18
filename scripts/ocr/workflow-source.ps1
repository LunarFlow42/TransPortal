param([Parameter(Mandatory)][string]$ControlDirectory)

# This process owns the only editable target used by the desktop workflow tests.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Threading;
public static class WorkflowInput {
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort key, scan; public uint flags, time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int x, y; public uint data, flags, time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] struct UNION { [FieldOffset(0)] public KEYBDINPUT key; [FieldOffset(0)] public MOUSEINPUT mouse; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public UNION data; }
  [DllImport("user32.dll")] static extern uint SendInput(uint count, INPUT[] input, int size);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out RECT rect);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
  [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr window, int command);
  [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr window);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr process);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
  [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
  static INPUT Key(ushort key, bool up) { return new INPUT { type = 1, data = new UNION { key = new KEYBDINPUT { key = key, flags = up ? 2u : 0u } } }; }
  static void Send(params INPUT[] input) { if (SendInput((uint)input.Length, input, Marshal.SizeOf(typeof(INPUT))) != input.Length) throw new Exception("Test keyboard injection failed"); }
  public static void Copy(int repeats, bool hold) {
    // Keep the edit control's message loop free to process each key/copy.
    ThreadPool.QueueUserWorkItem(_ => {
      Send(Key(0x11, false)); Thread.Sleep(30);
      Send(Key(0x43, false)); Thread.Sleep(30);
      for (int i = 1; i < repeats; i++) { if (!hold) Send(Key(0x43, true)); Thread.Sleep(120); Send(Key(0x43, false)); Thread.Sleep(30); }
      Send(Key(0x43, true)); Thread.Sleep(20); Send(Key(0x11, true));
    });
  }
  public static void Escape() { Send(Key(0x1b, false), Key(0x1b, true)); }
  public static void FocusWindow(IntPtr window) {
    ShowWindow(window, 9);
    SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, 0x43);
    bool usedAlt = GetForegroundWindow() != window;
    if (usedAlt) Send(Key(0x12, false), Key(0x12, true));
    uint own = GetCurrentThreadId(), foreground = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    bool attached = foreground != 0 && own != foreground && AttachThreadInput(own, foreground, true);
    try { BringWindowToTop(window); SetForegroundWindow(window); }
    finally { if (attached) AttachThreadInput(own, foreground, false); }
    if (usedAlt) ThreadPool.QueueUserWorkItem(_ => { Thread.Sleep(40); Escape(); });
    if (GetForegroundWindow() == window) return;
    RECT rect; if (!GetWindowRect(window, out rect)) throw new Exception("Missing test window");
    int x = (int)((long)(rect.left + 100 - GetSystemMetrics(76)) * 65535 / (GetSystemMetrics(78) - 1));
    int y = (int)((long)(rect.top + 12 - GetSystemMetrics(77)) * 65535 / (GetSystemMetrics(79) - 1));
    Send(new INPUT { type = 0, data = new UNION { mouse = new MOUSEINPUT { x = x, y = y, flags = 0xC003 } } },
         new INPUT { type = 0, data = new UNION { mouse = new MOUSEINPUT { x = x, y = y, flags = 0xC005 } } });
  }
  public static int[] Geometry(IntPtr window) {
    RECT rect; GetWindowRect(window, out rect);
    return new int[] { rect.left, rect.top, rect.right, rect.bottom, GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79), IsWindowVisible(window) ? 1 : 0, IsIconic(window) ? 1 : 0 };
  }
}
'@

[System.Windows.Forms.Application]::SetHighDpiMode([System.Windows.Forms.HighDpiMode]::PerMonitorV2) | Out-Null

$workflowOriginalClipboard = [System.Windows.Forms.DataObject]::new()
$workflowClipboardAvailable = $false
try {
    $workflowClipboardSource = [System.Windows.Forms.Clipboard]::GetDataObject()
    if ($workflowClipboardSource) {
        foreach ($workflowFormat in $workflowClipboardSource.GetFormats($false)) {
            $workflowValue = $workflowClipboardSource.GetData($workflowFormat, $false)
            if ($workflowValue -is [System.Drawing.Bitmap]) { $workflowValue = $workflowValue.Clone() }
            if ($workflowValue -is [System.IO.MemoryStream]) { $workflowValue = [System.IO.MemoryStream]::new($workflowValue.ToArray()) }
            if ($null -ne $workflowValue) { $workflowOriginalClipboard.SetData($workflowFormat, $false, $workflowValue) }
        }
        $workflowClipboardAvailable = $true
    }
} catch { }
$script:workflowLastClipboardRevision = [WorkflowInput]::GetClipboardSequenceNumber()
$workflowForm = [System.Windows.Forms.Form]::new()
$workflowForm.Text = 'TransPortal Workflow Test Source'
$workflowForm.StartPosition = 'Manual'
$workflowForm.Location = [System.Drawing.Point]::new(80, 80)
$workflowForm.ClientSize = [System.Drawing.Size]::new(940, 330)
$workflowForm.TopMost = $true
$workflowForm.BackColor = [System.Drawing.Color]::White
$workflowLabel = [System.Windows.Forms.Label]::new()
$workflowLabel.Text = 'TransPortal · isolated workflow test'
$workflowLabel.SetBounds(20, 15, 900, 35)
$workflowLabel.Font = [System.Drawing.Font]::new('Segoe UI', 16)
$workflowForm.Controls.Add($workflowLabel)
$workflowText = [System.Windows.Forms.TextBox]::new()
$workflowText.Multiline = $true
$workflowText.ImeMode = 'Disable'
$workflowText.HideSelection = $true
$workflowText.SetBounds(20, 65, 900, 230)
$workflowText.Font = [System.Drawing.Font]::new('Segoe UI', 24)
$workflowText.Text = "Hello world.`r`nPlease translate this selection."
$workflowForm.Controls.Add($workflowText)
$workflowOther = [System.Windows.Forms.Form]::new()
$workflowOther.Text = 'TransPortal Workflow Other Target'
$workflowOther.StartPosition = 'Manual'
$workflowOther.Location = [System.Drawing.Point]::new(50, 50)
$workflowOther.Size = [System.Drawing.Size]::new(350, 130)
$workflowOther.TopMost = $true
$workflowOtherText = [System.Windows.Forms.TextBox]::new()
$workflowOtherText.Text = 'Do not replace this text.'
$workflowOtherText.Dock = 'Fill'
$workflowOther.Controls.Add($workflowOtherText)
$script:workflowLastCommand = ''
$workflowTimer = [System.Windows.Forms.Timer]::new()
$workflowTimer.Interval = 40
$workflowTimer.Add_Tick({
    $workflowCommandPath = Join-Path $ControlDirectory 'command.json'
    if (-not (Test-Path -LiteralPath $workflowCommandPath)) { return }
    try { $workflowCommand = Get-Content -LiteralPath $workflowCommandPath -Raw | ConvertFrom-Json } catch { return }
    if ($workflowCommand.id -eq $script:workflowLastCommand) { return }
    $script:workflowLastCommand = $workflowCommand.id
    $workflowReply = @{ id = $workflowCommand.id }
    try {
        switch ($workflowCommand.operation) {
            'readonly' { $workflowText.ReadOnly = [bool]$workflowCommand.enabled }
            'activate' { $workflowOther.Hide(); [WorkflowInput]::FocusWindow($workflowForm.Handle) }
            'text' { $workflowText.Text = $workflowCommand.text; $workflowText.Select(0, 0) }
            'select' {
                $workflowOther.Hide()
                $workflowForm.Activate()
                [WorkflowInput]::SetForegroundWindow($workflowForm.Handle) | Out-Null
                $workflowText.Focus() | Out-Null
                $workflowText.Select([int]$workflowCommand.start, [int]$workflowCommand.length)
            }
            'copy' { [WorkflowInput]::Copy([int]$workflowCommand.repeats, [bool]$workflowCommand.hold) }
            'escape' { [WorkflowInput]::Escape() }
            'undo' { $workflowText.Undo() }
            'other' { $workflowOther.Show(); $workflowOther.Activate(); $workflowOtherText.Focus() | Out-Null; [WorkflowInput]::FocusWindow($workflowOther.Handle) }
            'clipboard' {
                if ($null -ne $workflowCommand.text) {
                    $workflowData = [System.Windows.Forms.DataObject]::new()
                    $workflowData.SetText([string]$workflowCommand.text)
                    $workflowData.SetData('TransPortal.WorkflowMarker', [string]'preserve me')
                    [System.Windows.Forms.Clipboard]::SetDataObject($workflowData, $true)
                }
                $workflowReply.clipboard = [System.Windows.Forms.Clipboard]::GetText()
                $workflowReply.marker = [System.Windows.Forms.Clipboard]::ContainsData('TransPortal.WorkflowMarker')
            }
            'quit' { $workflowForm.Close() }
        }
        $workflowPoint = $workflowText.PointToScreen([System.Drawing.Point]::new(5, 5))
        $workflowReply.text = $workflowText.Text
        $workflowReply.region = @{ x = $workflowPoint.X; y = $workflowPoint.Y; width = $workflowText.ClientSize.Width - 10; height = $workflowText.ClientSize.Height - 10 }
        $workflowReply.window = $workflowForm.Handle.ToInt64()
        $workflowReply.foreground = [WorkflowInput]::GetForegroundWindow().ToInt64()
        $workflowReply.geometry = [WorkflowInput]::Geometry($workflowForm.Handle)
        $workflowReply.selectionLength = $workflowText.SelectionLength
        $workflowReply.textFocused = $workflowText.Focused
        $script:workflowLastClipboardRevision = [WorkflowInput]::GetClipboardSequenceNumber()
    } catch { $workflowReply.error = $_.Exception.Message }
    $workflowReply | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $ControlDirectory 'reply.json') -Encoding utf8
})
try {
    $workflowTimer.Start()
    [System.Windows.Forms.Application]::Run($workflowForm)
} finally {
    $workflowTimer.Stop()
    if ($workflowClipboardAvailable -and [WorkflowInput]::GetClipboardSequenceNumber() -eq $script:workflowLastClipboardRevision) {
        try { [System.Windows.Forms.Clipboard]::SetDataObject($workflowOriginalClipboard, $true) } catch { }
    }
    $workflowOther.Dispose()
    $workflowForm.Dispose()
    $workflowTimer.Dispose()
}
