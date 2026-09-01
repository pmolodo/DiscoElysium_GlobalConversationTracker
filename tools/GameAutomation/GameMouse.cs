// SPDX-License-Identifier: MIT
using System;
using System.Runtime.InteropServices;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Moves and clicks the mouse, for the one thing the keyboard cannot do: start a
    /// conversation.
    /// </summary>
    /// <remarks>
    /// <para>The look-ahead only runs when a response menu is drawn, and nothing draws
    /// one without an NPC being clicked. Disco Elysium is a point-and-click game; there
    /// is no key that opens a dialogue.</para>
    ///
    /// <para>Absolute positioning, never relative. A relative move is applied after
    /// pointer acceleration, so the same request lands somewhere different depending on
    /// where the cursor already was and what the player's mouse settings are - and a
    /// click that lands a few pixels off in this game hits the ground and walks there
    /// instead, which is not distinguishable from "the NPC ignored us".</para>
    ///
    /// <para>Every move is read back with <c>GetCursorPos</c> and refused if it did not
    /// land, for the same reason <see cref="GameKeyboard"/> checks SendInput's return:
    /// injected input that quietly goes nowhere looks exactly like a game that ignored
    /// it, and the two want completely different fixes.</para>
    ///
    /// <para>Compiled rather than scripted, like the keyboard: SendInput beside window
    /// enumeration and screen capture is what Defender's AMSI blocks in a script, and
    /// fairly so.</para>
    /// </remarks>
    public static class GameMouse
    {
        [StructLayout(LayoutKind.Sequential)]
        private struct MouseInput
        {
            public int X;
            public int Y;
            public uint MouseData;
            public uint Flags;
            public uint Time;
            public IntPtr ExtraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Input
        {
            public uint Type;
            public MouseInput Mouse;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Point
        {
            public int X;
            public int Y;
        }

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint count, Input[] inputs, int size);

        [DllImport("user32.dll")]
        private static extern bool GetCursorPos(out Point point);

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int index);

        private const uint InputMouse = 0;
        private const uint MoveFlag = 0x0001;
        private const uint LeftDown = 0x0002;
        private const uint LeftUp = 0x0004;
        private const uint RightDown = 0x0008;
        private const uint RightUp = 0x0010;
        private const uint AbsoluteFlag = 0x8000;
        private const uint VirtualDeskFlag = 0x4000;

        private const int SmXVirtualScreen = 76;
        private const int SmYVirtualScreen = 77;
        private const int SmCxVirtualScreen = 78;
        private const int SmCyVirtualScreen = 79;

        /// <summary>
        /// The largest a verified move may miss by, in pixels.
        /// </summary>
        /// <remarks>
        /// Not zero. The absolute range is 16 bits stretched over the virtual desktop, so
        /// a pixel on a wide desktop is not exactly representable and the driver rounds.
        /// One pixel of slack accepts that rounding and still catches a move that was
        /// swallowed, clamped to a monitor edge, or applied to the wrong screen.
        /// </remarks>
        public const int LandingTolerance = 1;

        /// <summary>How long a click holds the button down.</summary>
        /// <remarks>
        /// Long enough that the game samples the button as down at least once. A press
        /// and release in the same frame can be missed entirely, and a missed click is
        /// silent.
        /// </remarks>
        public static readonly TimeSpan DefaultClickHold = TimeSpan.FromMilliseconds(60);

        /// <summary>How long to settle after a move before clicking.</summary>
        /// <remarks>
        /// The game highlights what is under the cursor on hover, and decides what a
        /// click means from that highlight. Clicking in the same instant as the move
        /// can be resolved against wherever the cursor was before.
        /// </remarks>
        public static readonly TimeSpan DefaultSettle = TimeSpan.FromMilliseconds(250);

        /// <summary>Where the cursor is now, in screen coordinates.</summary>
        /// <exception cref="InvalidOperationException">Windows would not answer.</exception>
        public static System.Drawing.Point Position
        {
            get
            {
                if (!GetCursorPos(out Point point))
                {
                    throw new InvalidOperationException(
                        $"GetCursorPos failed (error {Marshal.GetLastWin32Error()}).");
                }

                return new System.Drawing.Point(point.X, point.Y);
            }
        }

        /// <summary>
        /// Maps one screen coordinate onto SendInput's absolute 0..65535 axis.
        /// </summary>
        /// <remarks>
        /// The axis spans the whole virtual desktop, so a coordinate is placed relative
        /// to its origin rather than to zero - on a multi-monitor desktop the origin is
        /// negative whenever a second monitor sits left of or above the primary one, and
        /// ignoring it puts every click on the wrong screen.
        /// </remarks>
        /// <param name="coordinate">The screen coordinate.</param>
        /// <param name="origin">The virtual desktop's origin on that axis.</param>
        /// <param name="size">The virtual desktop's size on that axis.</param>
        /// <returns>The value SendInput wants, 0..65535.</returns>
        /// <exception cref="ArgumentOutOfRangeException">The size is not positive.</exception>
        public static int ToAbsolute(int coordinate, int origin, int size)
        {
            if (size <= 0)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(size), size, "The virtual desktop cannot be empty.");
            }

            if (size == 1)
            {
                return 0;
            }

            // Divided by size - 1, not size, so the last pixel maps to the top of the
            // range rather than one step short of it. Rounded rather than truncated: on a
            // wide desktop truncation biases every click a pixel left of where it was
            // asked for, which is invisible until something small is being aimed at.
            long scaled = ((long)(coordinate - origin) * 65535) + ((size - 1) / 2);
            long absolute = scaled / (size - 1);
            return (int)Math.Max(0, Math.Min(65535, absolute));
        }

        /// <summary>Moves the cursor and confirms it arrived.</summary>
        /// <param name="x">Screen x.</param>
        /// <param name="y">Screen y.</param>
        /// <exception cref="InvalidOperationException">
        /// Windows refused the injection, or the cursor is not where it was sent.
        /// </exception>
        public static void MoveTo(int x, int y)
        {
            int originX = GetSystemMetrics(SmXVirtualScreen);
            int originY = GetSystemMetrics(SmYVirtualScreen);
            int width = GetSystemMetrics(SmCxVirtualScreen);
            int height = GetSystemMetrics(SmCyVirtualScreen);

            var input = new Input
            {
                Type = InputMouse,
                Mouse = new MouseInput
                {
                    X = ToAbsolute(x, originX, width),
                    Y = ToAbsolute(y, originY, height),
                    Flags = MoveFlag | AbsoluteFlag | VirtualDeskFlag,
                },
            };

            Send(input, $"the move to {x},{y}");

            // The move is asynchronous: it is posted to the input queue, so the cursor
            // has not necessarily arrived by the time SendInput returns.
            Thread.Sleep(20);

            System.Drawing.Point landed = Position;
            if (Math.Abs(landed.X - x) > LandingTolerance
                || Math.Abs(landed.Y - y) > LandingTolerance)
            {
                throw new InvalidOperationException(
                    $"Asked the cursor to {x},{y} but it is at {landed.X},{landed.Y}. The point "
                    + $"may be off the virtual desktop ({width}x{height} at {originX},{originY}).");
            }
        }

        /// <summary>Presses and releases a button where the cursor already is.</summary>
        /// <param name="button">"Left" or "Right".</param>
        /// <param name="hold">How long to hold it down.</param>
        /// <exception cref="ArgumentException">The button name is not recognised.</exception>
        public static void Click(string button = "Left", TimeSpan? hold = null)
        {
            bool right = string.Equals(button, "Right", StringComparison.OrdinalIgnoreCase);
            if (!right && !string.Equals(button, "Left", StringComparison.OrdinalIgnoreCase))
            {
                throw new ArgumentException(
                    $"Unknown mouse button '{button}'. Known buttons: Left, Right.",
                    nameof(button));
            }

            Send(ButtonInput(right ? RightDown : LeftDown), $"the {button} button going down");
            Thread.Sleep(hold ?? DefaultClickHold);
            Send(ButtonInput(right ? RightUp : LeftUp), $"the {button} button coming up");
        }

        /// <summary>Moves to a point inside a window's client area and clicks it.</summary>
        /// <param name="window">The window to click in.</param>
        /// <param name="clientX">X within the client area.</param>
        /// <param name="clientY">Y within the client area.</param>
        /// <param name="button">"Left" or "Right".</param>
        /// <param name="settle">How long to hover before clicking.</param>
        /// <exception cref="ArgumentNullException"><paramref name="window"/> is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException">The point is outside the window.</exception>
        /// <exception cref="InvalidOperationException">Windows would not place the cursor.</exception>
        public static void ClickInWindow(
            GameWindow window,
            int clientX,
            int clientY,
            string button = "Left",
            TimeSpan? settle = null)
        {
            if (window == null)
            {
                throw new ArgumentNullException(nameof(window));
            }

            if (clientX < 0 || clientX >= window.Width || clientY < 0 || clientY >= window.Height)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(clientX),
                    $"{clientX},{clientY} is outside the {window.Width}x{window.Height} client area.");
            }

            if (!GameWindows.TryGetClientBounds(window.Handle, out int x, out int y, out _, out _))
            {
                throw new InvalidOperationException(
                    "Could not find where the game's client area is on screen.");
            }

            MoveTo(x + clientX, y + clientY);
            Thread.Sleep(settle ?? DefaultSettle);
            Click(button);
        }

        private static Input ButtonInput(uint flags)
        {
            return new Input
            {
                Type = InputMouse,
                Mouse = new MouseInput { Flags = flags },
            };
        }

        private static void Send(Input input, string what)
        {
            uint sent = SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input)));
            if (sent != 1)
            {
                int error = Marshal.GetLastWin32Error();
                throw new InvalidOperationException(
                    $"SendInput refused {what} (error {error}). A more privileged window in the "
                    + "foreground blocks injected input; run the harness elevated, or close it.");
            }
        }
    }
}
