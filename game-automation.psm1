# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Screenshots and keyboard input for driving the game from a script.

.DESCRIPTION
    Two capabilities, both without a dependency, because the alternative packages
    each bring a problem this does not need.

    KEYBOARD. SendInput with SCANCODES, not virtual keys. A game reading raw
    input or DirectInput ignores virtual-key injection and SendKeys entirely
    (https://www.gamedev.net/forums/topic/581515-simulating-keyboard-with-sendinput-api-in-directinput-applications/),
    which is why Windows Forms' SendKeys is no use here - it simulates text entry
    rather than keystrokes
    (https://learn.microsoft.com/en-us/dotnet/desktop/winforms/input-keyboard/how-to-simulate-events).
    The release event must set KEYEVENTF_KEYUP *and* KEYEVENTF_SCANCODE together;
    sending KEYUP without the scancode flag is the well-known way to leave keys
    stuck down.

    SCREENSHOTS. Graphics.CopyFromScreen over the window's client rect, not
    PrintWindow. PrintWindow asks the window to paint itself, which a
    GPU-rendered Unity window answers with black. Capturing the screen means the
    window has to be visible and foregrounded, which is a real constraint but an
    honest one.

    Comparison is a mean absolute difference over a downscaled greyscale copy,
    reported 0..1. Downscaling first makes it robust to a cursor or a flickering
    pixel and makes a comparison cheap enough to poll with.

.NOTES
    Everything here needs the game window focused and unobstructed. A screensaver,
    a lock screen, or another window on top will silently produce a screenshot of
    the wrong thing - which is why Wait-GameScreen returns the difference it saw
    rather than just a boolean, so a caller can tell "never matched" from
    "matched immediately".
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

if (-not ('GctNative' -as [type])) {
    Add-Type -Namespace '' -Name 'GctNative' -MemberDefinition @'
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    // INPUT is a union; the keyboard arm is the largest member we use, and the
    // padding keeps the struct the size SendInput expects on 64-bit.
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT {
        public uint type;
        public KEYBDINPUT ki;
        public int padding1;
        public int padding2;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);
'@
}

# Scancodes (set 1) for the keys a menu needs. Deliberately short: every entry
# here is one somebody has a reason to press, and an unknown name is an error
# rather than a silent no-op.
$script:ScanCodes = @{
    'Escape' = 0x01; 'Enter' = 0x1C; 'Space' = 0x39; 'Tab' = 0x0F
    'Backspace' = 0x0E
    'Up' = 0x48; 'Down' = 0x50; 'Left' = 0x4B; 'Right' = 0x4D
    'Home' = 0x47; 'End' = 0x4F; 'PageUp' = 0x49; 'PageDown' = 0x51
    'F1' = 0x3B; 'F2' = 0x3C; 'F3' = 0x3D; 'F4' = 0x3E; 'F5' = 0x3F; 'F6' = 0x40
    'F7' = 0x41; 'F8' = 0x42; 'F9' = 0x43; 'F10' = 0x44; 'F11' = 0x57; 'F12' = 0x58
    '1' = 0x02; '2' = 0x03; '3' = 0x04; '4' = 0x05; '5' = 0x06
    '6' = 0x07; '7' = 0x08; '8' = 0x09; '9' = 0x0A; '0' = 0x0B
    'A' = 0x1E; 'B' = 0x30; 'C' = 0x2E; 'D' = 0x20; 'E' = 0x12; 'F' = 0x21
    'G' = 0x22; 'H' = 0x23; 'I' = 0x17; 'J' = 0x24; 'K' = 0x25; 'L' = 0x26
    'M' = 0x32; 'N' = 0x31; 'O' = 0x18; 'P' = 0x19; 'Q' = 0x10; 'R' = 0x13
    'S' = 0x1F; 'T' = 0x14; 'U' = 0x16; 'V' = 0x2F; 'W' = 0x11; 'X' = 0x2D
    'Y' = 0x15; 'Z' = 0x2C
}

# The arrow, navigation and keypad keys are "extended" - their scancode needs the
# E0 prefix flag or the game reads a numeric-keypad key instead.
$script:ExtendedKeys = @(
    'Up', 'Down', 'Left', 'Right', 'Home', 'End', 'PageUp', 'PageDown'
)

$script:INPUT_KEYBOARD = 1
$script:KEYEVENTF_KEYUP = 0x0002
$script:KEYEVENTF_SCANCODE = 0x0008
$script:KEYEVENTF_EXTENDEDKEY = 0x0001
$script:SW_RESTORE = 9

function Get-GameKeyName {
    <#
    .SYNOPSIS
        Every key name Send-GameKey accepts.
    #>
    [CmdletBinding()]
    param()

    return ($script:ScanCodes.Keys | Sort-Object)
}

function Find-GameWindow {
    <#
    .SYNOPSIS
        The game's main window handle.

    .PARAMETER ProcessName
        The process to look for, without the .exe.
    #>
    [CmdletBinding()]
    param(
        [string] $ProcessName = 'disco'
    )

    $candidates = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 })

    if ($candidates.Count -eq 0) {
        return $null
    }
    if ($candidates.Count -gt 1) {
        throw "$($candidates.Count) '$ProcessName' processes have a main window; cannot tell which is the game."
    }

    return [pscustomobject]@{
        Handle  = $candidates[0].MainWindowHandle
        Process = $candidates[0]
        Title   = $candidates[0].MainWindowTitle
    }
}

function Wait-GameWindow {
    <#
    .SYNOPSIS
        Waits for the game's window to exist.
    #>
    [CmdletBinding()]
    param(
        [string] $ProcessName = 'disco',
        [int] $TimeoutSeconds = 120
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $window = Find-GameWindow -ProcessName $ProcessName
        if ($window -and [GctNative]::IsWindowVisible($window.Handle)) {
            return $window
        }

        Start-Sleep -Milliseconds 500
    }

    throw "No visible '$ProcessName' window appeared within $TimeoutSeconds seconds."
}

function Set-GameWindowForeground {
    <#
    .SYNOPSIS
        Brings the game to the front, which both input and capture require.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Window
    )

    [void][GctNative]::ShowWindow($Window.Handle, $script:SW_RESTORE)
    [void][GctNative]::SetForegroundWindow($Window.Handle)
    Start-Sleep -Milliseconds 300

    return ([GctNative]::GetForegroundWindow() -eq $Window.Handle)
}

function Get-GameWindowRect {
    <#
    .SYNOPSIS
        The window's client area in screen coordinates.

    .DESCRIPTION
        The client rect, not the window rect, so a title bar and borders stay out
        of the comparison - they do not change when the game does.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Window
    )

    $rect = New-Object GctNative+RECT
    if (-not [GctNative]::GetClientRect($Window.Handle, [ref] $rect)) {
        throw 'GetClientRect failed.'
    }

    $origin = New-Object GctNative+POINT
    $origin.X = 0
    $origin.Y = 0
    if (-not [GctNative]::ClientToScreen($Window.Handle, [ref] $origin)) {
        throw 'ClientToScreen failed.'
    }

    return [pscustomobject]@{
        X      = $origin.X
        Y      = $origin.Y
        Width  = $rect.Right - $rect.Left
        Height = $rect.Bottom - $rect.Top
    }
}

function Get-GameScreenshot {
    <#
    .SYNOPSIS
        Captures the game's client area.

    .DESCRIPTION
        CopyFromScreen, so the window must be visible and on top. Returns a
        Bitmap the caller is responsible for disposing, or writes a PNG.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Window,
        [string] $Path
    )

    $rect = Get-GameWindowRect -Window $Window
    if ($rect.Width -le 0 -or $rect.Height -le 0) {
        throw "The window has no client area ($($rect.Width)x$($rect.Height)); is it minimised?"
    }

    $bitmap = New-Object System.Drawing.Bitmap($rect.Width, $rect.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen(
            $rect.X, $rect.Y, 0, 0,
            (New-Object System.Drawing.Size($rect.Width, $rect.Height)))
    }
    finally {
        $graphics.Dispose()
    }

    if ($Path) {
        $directory = Split-Path -Parent $Path
        if ($directory -and -not (Test-Path -LiteralPath $directory)) {
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
        }

        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    }

    return $bitmap
}

function ConvertTo-ScreenshotSignature {
    <#
    .SYNOPSIS
        A small greyscale fingerprint of an image, for comparing cheaply.

    .DESCRIPTION
        Downscaling first is what makes a comparison robust: a mouse cursor, a
        blinking caret or one flickering pixel moves a full-resolution difference
        far more than it moves this. GetPixel on a 32x32 grid is 1024 reads, which
        is fast enough to poll several times a second.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [System.Drawing.Bitmap] $Bitmap,
        [int] $Size = 32
    )

    $small = New-Object System.Drawing.Bitmap($Size, $Size)
    $graphics = [System.Drawing.Graphics]::FromImage($small)
    try {
        $graphics.InterpolationMode =
            [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBilinear
        $graphics.DrawImage($Bitmap, 0, 0, $Size, $Size)
    }
    finally {
        $graphics.Dispose()
    }

    $values = New-Object 'double[]' ($Size * $Size)
    for ($y = 0; $y -lt $Size; $y++) {
        for ($x = 0; $x -lt $Size; $x++) {
            $pixel = $small.GetPixel($x, $y)
            $values[($y * $Size) + $x] =
                (0.299 * $pixel.R + 0.587 * $pixel.G + 0.114 * $pixel.B) / 255.0
        }
    }

    $small.Dispose()
    return $values
}

function Compare-Screenshot {
    <#
    .SYNOPSIS
        How different two images are, from 0 (identical) to 1 (black vs white).

    .DESCRIPTION
        Mean absolute difference over the fingerprints. A mean rather than a max
        so that one changed region does not saturate the number, which is what
        makes a threshold meaningful.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $First,
        [Parameter(Mandatory)] $Second,
        [int] $Size = 32
    )

    $a = if ($First -is [System.Drawing.Bitmap]) {
        ConvertTo-ScreenshotSignature -Bitmap $First -Size $Size
    } else { $First }

    $b = if ($Second -is [System.Drawing.Bitmap]) {
        ConvertTo-ScreenshotSignature -Bitmap $Second -Size $Size
    } else { $Second }

    if ($a.Count -ne $b.Count) {
        throw "Fingerprints are different sizes ($($a.Count) vs $($b.Count)); compare at one Size."
    }

    $total = 0.0
    for ($i = 0; $i -lt $a.Count; $i++) {
        $total += [Math]::Abs($a[$i] - $b[$i])
    }

    return $total / $a.Count
}

function Wait-GameScreenStable {
    <#
    .SYNOPSIS
        Waits until the screen stops changing.

    .DESCRIPTION
        Loading screens move; a settled menu mostly does not. "Mostly" is why this
        takes a threshold rather than testing for equality - Disco Elysium's main
        menu animates a diorama behind the buttons, so consecutive frames are
        never identical.

        Returns what it saw, so a caller can tell a real settle from a timeout.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Window,
        [double] $Threshold = 0.02,
        [int] $StableSamples = 4,
        [int] $IntervalMilliseconds = 500,
        [int] $TimeoutSeconds = 300
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $previous = $null
    $stable = 0
    $last = 1.0

    while ((Get-Date) -lt $deadline) {
        $shot = Get-GameScreenshot -Window $Window
        try {
            $signature = ConvertTo-ScreenshotSignature -Bitmap $shot
        }
        finally {
            $shot.Dispose()
        }

        if ($previous) {
            $last = Compare-Screenshot -First $previous -Second $signature
            if ($last -le $Threshold) {
                $stable++
                Write-Verbose ("stable {0}/{1} (difference {2:N4})" -f $stable, $StableSamples, $last)
                if ($stable -ge $StableSamples) {
                    return [pscustomobject]@{
                        Settled    = $true
                        Difference = $last
                        Waited     = $TimeoutSeconds - [int]($deadline - (Get-Date)).TotalSeconds
                    }
                }
            }
            else {
                if ($stable -gt 0) { Write-Verbose "changed again (difference $last)" }
                $stable = 0
            }
        }

        $previous = $signature
        Start-Sleep -Milliseconds $IntervalMilliseconds
    }

    return [pscustomobject]@{
        Settled    = $false
        Difference = $last
        Waited     = $TimeoutSeconds
    }
}

function Wait-GameScreen {
    <#
    .SYNOPSIS
        Waits until the screen matches a reference image.

    .DESCRIPTION
        For "we are at the main menu", where a settle alone cannot tell one still
        screen from another. Capture the reference once with Get-GameScreenshot
        and keep it beside the test.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Window,
        [Parameter(Mandatory)] [string] $ReferencePath,
        [double] $Threshold = 0.05,
        [int] $IntervalMilliseconds = 500,
        [int] $TimeoutSeconds = 300
    )

    if (-not (Test-Path -LiteralPath $ReferencePath)) {
        throw "No reference image at $ReferencePath."
    }

    $reference = [System.Drawing.Bitmap]::FromFile($ReferencePath)
    try {
        $referenceSignature = ConvertTo-ScreenshotSignature -Bitmap $reference
    }
    finally {
        $reference.Dispose()
    }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $best = 1.0

    while ((Get-Date) -lt $deadline) {
        $shot = Get-GameScreenshot -Window $Window
        try {
            $signature = ConvertTo-ScreenshotSignature -Bitmap $shot
        }
        finally {
            $shot.Dispose()
        }

        $difference = Compare-Screenshot -First $referenceSignature -Second $signature
        if ($difference -lt $best) { $best = $difference }
        Write-Verbose ("difference from reference: {0:N4} (best {1:N4})" -f $difference, $best)

        if ($difference -le $Threshold) {
            return [pscustomobject]@{ Matched = $true; Difference = $difference; Best = $best }
        }

        Start-Sleep -Milliseconds $IntervalMilliseconds
    }

    return [pscustomobject]@{ Matched = $false; Difference = $difference; Best = $best }
}

function Send-GameKey {
    <#
    .SYNOPSIS
        Presses and releases a key, as a scancode, through SendInput.

    .DESCRIPTION
        Scancodes because a game reading raw input or DirectInput ignores
        virtual-key injection. The release ORs KEYEVENTF_KEYUP with
        KEYEVENTF_SCANCODE: dropping the scancode flag on the release is the
        documented way to leave a key stuck down.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory, Position = 0)] [string] $Key,
        [int] $HoldMilliseconds = 40,
        [int] $AfterMilliseconds = 120
    )

    if (-not $script:ScanCodes.ContainsKey($Key)) {
        throw "Unknown key '$Key'. Known keys: $((Get-GameKeyName) -join ', ')."
    }

    $scan = $script:ScanCodes[$Key]
    $extended = if ($script:ExtendedKeys -contains $Key) { $script:KEYEVENTF_EXTENDEDKEY } else { 0 }

    $down = New-Object GctNative+INPUT
    $down.type = $script:INPUT_KEYBOARD
    $down.ki.wVk = 0
    $down.ki.wScan = [ushort] $scan
    $down.ki.dwFlags = $script:KEYEVENTF_SCANCODE -bor $extended
    $down.ki.time = 0
    $down.ki.dwExtraInfo = [IntPtr]::Zero

    $up = New-Object GctNative+INPUT
    $up.type = $script:INPUT_KEYBOARD
    $up.ki.wVk = 0
    $up.ki.wScan = [ushort] $scan
    $up.ki.dwFlags = $script:KEYEVENTF_SCANCODE -bor $script:KEYEVENTF_KEYUP -bor $extended
    $up.ki.time = 0
    $up.ki.dwExtraInfo = [IntPtr]::Zero

    $size = [System.Runtime.InteropServices.Marshal]::SizeOf([type] 'GctNative+INPUT')

    $sent = [GctNative]::SendInput(1, @($down), $size)
    if ($sent -ne 1) {
        throw "SendInput refused the key-down for '$Key' (last error $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error()))."
    }

    Start-Sleep -Milliseconds $HoldMilliseconds

    $sent = [GctNative]::SendInput(1, @($up), $size)
    if ($sent -ne 1) {
        throw "SendInput refused the key-up for '$Key'; it may now be stuck down."
    }

    if ($AfterMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $AfterMilliseconds
    }
}

function Send-GameKeys {
    <#
    .SYNOPSIS
        Presses several keys in order.

    .EXAMPLE
        Send-GameKeys 'Escape', 'Down', 'Down', 'Enter'
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory, Position = 0)] [string[]] $Keys,
        [int] $HoldMilliseconds = 40,
        [int] $AfterMilliseconds = 120
    )

    foreach ($key in $Keys) {
        Write-Verbose "key: $key"
        Send-GameKey -Key $key -HoldMilliseconds $HoldMilliseconds `
            -AfterMilliseconds $AfterMilliseconds
    }
}

Export-ModuleMember -Function `
    Get-GameKeyName, `
    Find-GameWindow, `
    Wait-GameWindow, `
    Set-GameWindowForeground, `
    Get-GameWindowRect, `
    Get-GameScreenshot, `
    ConvertTo-ScreenshotSignature, `
    Compare-Screenshot, `
    Wait-GameScreenStable, `
    Wait-GameScreen, `
    Send-GameKey, `
    Send-GameKeys
