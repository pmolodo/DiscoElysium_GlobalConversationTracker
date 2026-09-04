// SPDX-License-Identifier: MIT
using System;
using System.Runtime.InteropServices;
using System.Drawing;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Keeps the display on for the length of a run, and wakes it if it is already off.
    /// </summary>
    /// <remarks>
    /// <para>An in-game run reads the screen. Every check it makes - is this the main menu,
    /// did the screen leave it, what does the response menu say - is a screenshot compared
    /// against a reference, and a screenshot of a powered-down display is blank. So is the
    /// window it wants to bring to the front: <c>SetForegroundWindow</c> on a machine whose
    /// display has gone to sleep does not do what it says.</para>
    ///
    /// <para>The failure that looks like: the run reports that it cannot read the screen,
    /// that raising the window failed, and that the difference against the main-menu
    /// reference is 1.0 with a DETAIL OF ZERO. Detail near zero means the capture had almost
    /// no variation in it, which is what a blank frame is; a run that has genuinely landed
    /// on the wrong screen has plenty of detail and simply does not match.</para>
    ///
    /// <para>TWO DIFFERENT PROBLEMS, and this covers both. Preventing sleep is
    /// <c>SetThreadExecutionState</c>, the same call a media player makes so a film is not
    /// interrupted; it holds for as long as the process lives and does nothing to a display
    /// that is already off. Waking one that already is means giving the system some input,
    /// which is what <see cref="Wake"/> does.</para>
    ///
    /// <para>WHAT THIS CANNOT DO is unlock a locked session. A locked desktop cannot be
    /// captured or driven by anything running in the user's session, and no amount of
    /// synthetic input changes that - it is the point of locking. A run that fails with a
    /// blank screen on a LOCKED machine needs a person, and saying so is better than
    /// pretending otherwise.</para>
    /// </remarks>
    public sealed class DisplayAwake : IDisposable
    {
        [Flags]
        private enum ExecutionState : uint
        {
            /// <summary>The request stands until it is cleared, rather than resetting one idle timer.</summary>
            Continuous = 0x80000000,

            /// <summary>Keep the display on.</summary>
            DisplayRequired = 0x00000002,

            /// <summary>Keep the machine out of sleep.</summary>
            SystemRequired = 0x00000001,
        }

        private bool _released;

        private DisplayAwake()
        {
        }

        /// <summary>
        /// Asks the system to keep the display and the machine awake until this is
        /// disposed.
        /// </summary>
        /// <remarks>
        /// Returns an object even when the request was refused. There is nothing useful for
        /// a caller to do about it - the run is worth attempting either way, and it will
        /// say clearly enough if the screen turns out to be blank - and a run that threw
        /// here would fail for a reason nobody asked about.
        /// </remarks>
        public static DisplayAwake Keep()
        {
            // Non-zero is the previous state; zero means the request was refused.
            SetThreadExecutionState(
                ExecutionState.Continuous
                | ExecutionState.DisplayRequired
                | ExecutionState.SystemRequired);

            return new DisplayAwake();
        }

        /// <summary>Whether the system accepted the request to stay awake.</summary>
        /// <remarks>
        /// Asked by re-issuing the same request, whose return value is the state that was
        /// in force before it. Cheap, and there is no other way to read it back.
        /// </remarks>
        public static bool IsHeld()
        {
            uint previous = SetThreadExecutionState(
                ExecutionState.Continuous
                | ExecutionState.DisplayRequired
                | ExecutionState.SystemRequired);
            return previous != 0;
        }

        /// <summary>
        /// Wakes a display that has already gone off, by giving the system some input.
        /// </summary>
        /// <remarks>
        /// <para>A mouse move of zero distance. It counts as input for the purposes of the
        /// idle timer and the display's power state, and it moves the pointer nowhere, so
        /// it cannot land on anything or drag anything - which matters because this runs
        /// against a game that is about to be driven by real clicks.</para>
        ///
        /// <para>Not <c>WM_SYSCOMMAND</c>/<c>SC_MONITORPOWER</c>, which is the other usual
        /// answer: broadcasting it turns displays ON, and broadcasting the value that turns
        /// them OFF is one typo away. Input has no such neighbouring mistake.</para>
        /// </remarks>
        public static void Wake()
        {
            var input = new Input
            {
                Type = InputMouse,
                Data = new InputUnion
                {
                    Mouse = new MouseInput
                    {
                        Dx = 0,
                        Dy = 0,
                        MouseData = 0,
                        Flags = MouseEventMove,
                        Time = 0,
                        ExtraInfo = IntPtr.Zero,
                    },
                },
            };

            // A refused send is not worth failing over: the caller is about to find out
            // whether the screen is readable, which is the question that actually matters.
            SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input)));
        }

        /// <summary>
        /// Wakes the display if it is asleep, and says whether the screen can be read
        /// afterwards.
        /// </summary>
        /// <remarks>
        /// <para>THE ORDER MATTERS. Keeping the display awake does nothing for one that is
        /// already off, and an in-game run started from an idle machine meets exactly that:
        /// the display went to sleep some time ago and every screenshot the run is about to
        /// take is blank. So this nudges first and then CHECKS, rather than asking and
        /// hoping.</para>
        ///
        /// <para>Checked by reading the screen, because Windows has no straight answer to
        /// "is the display on". A desktop capture of a sleeping display is blank, and blank
        /// is measurable: the same detail figure the run already uses to tell a screen with
        /// something on it from one without. That also means this cannot tell a sleeping
        /// display from a LOCKED session, and it does not pretend to - both read as blank,
        /// and only one of them can be fixed from in here.</para>
        /// </remarks>
        /// <param name="report">Where progress is described, or null for silence.</param>
        /// <param name="timeout">How long to keep trying before giving up.</param>
        /// <returns>False if the screen still reads as blank when the time is up.</returns>
        public static bool WakeAndCheck(Action<string>? report, TimeSpan timeout)
        {
            if (Readable())
            {
                return true;
            }

            report?.Invoke("the screen reads as blank; waking the display...");

            var elapsed = System.Diagnostics.Stopwatch.StartNew();
            while (elapsed.Elapsed < timeout)
            {
                Wake();

                // A display takes a moment to come back, and asking again immediately
                // measures the dark it was already in.
                System.Threading.Thread.Sleep(WakeSettleMilliseconds);
                if (Readable())
                {
                    report?.Invoke($"the display woke after {elapsed.Elapsed.TotalSeconds:N0}s.");
                    return true;
                }
            }

            report?.Invoke(
                $"the screen still reads as blank after {timeout.TotalSeconds:N0}s. Either the "
                + "display will not wake or THE SESSION IS LOCKED, which nothing running "
                + "inside it can undo - an in-game run needs somebody to unlock the machine.");
            return false;
        }

        /// <summary>Whether the desktop has anything on it worth calling a picture.</summary>
        /// <remarks>
        /// The whole primary screen rather than any one window, because this runs before the
        /// game has been launched and there is no window of ours to look at yet.
        /// </remarks>
        public static bool Readable()
        {
            try
            {
                using Bitmap desktop = CaptureDesktop();
                return GameScreen.Detail(GameScreen.FingerprintOf(desktop))
                    >= GameSession.BlankDetailFloor;
            }
            catch (Exception)
            {
                // A capture that will not run at all is its own answer, and it is the same
                // answer: nothing here can read the screen.
                return false;
            }
        }

        /// <summary>The primary screen, as an image.</summary>
        /// <remarks>
        /// Sized through <c>GetSystemMetrics</c> rather than through Windows Forms, which
        /// this project does not reference and should not start referencing to ask how wide
        /// a screen is.
        /// </remarks>
        private static Bitmap CaptureDesktop()
        {
            int width = GetSystemMetrics(PrimaryScreenWidth);
            int height = GetSystemMetrics(PrimaryScreenHeight);
            if (width <= 0 || height <= 0)
            {
                throw new InvalidOperationException(
                    $"Windows reports a {width}x{height} primary screen.");
            }

            var bitmap = new Bitmap(width, height);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.CopyFromScreen(0, 0, 0, 0, new Size(width, height));
            }

            return bitmap;
        }

        private const int PrimaryScreenWidth = 0;
        private const int PrimaryScreenHeight = 1;

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int index);

        /// <summary>How long to let a display come back before looking again.</summary>
        private const int WakeSettleMilliseconds = 750;

        /// <summary>Lets the display sleep again.</summary>
        public void Dispose()
        {
            if (_released)
            {
                return;
            }

            _released = true;
            SetThreadExecutionState(ExecutionState.Continuous);
        }

        private const int InputMouse = 0;
        private const uint MouseEventMove = 0x0001;

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint SetThreadExecutionState(ExecutionState flags);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint count, Input[] inputs, int size);

        [StructLayout(LayoutKind.Sequential)]
        private struct Input
        {
            public int Type;
            public InputUnion Data;
        }

        [StructLayout(LayoutKind.Explicit)]
        private struct InputUnion
        {
            [FieldOffset(0)]
            public MouseInput Mouse;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MouseInput
        {
            public int Dx;
            public int Dy;
            public uint MouseData;
            public uint Flags;
            public uint Time;
            public IntPtr ExtraInfo;
        }
    }
}
