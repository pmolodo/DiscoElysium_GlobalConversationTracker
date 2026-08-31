// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace GlobalConversationTracker.Automation
{
    /// <summary>One top-level window, as the automation needs to see it.</summary>
    public sealed class GameWindow
    {
        /// <summary>Creates a description.</summary>
        /// <param name="handle">The window handle.</param>
        /// <param name="processId">The process that owns it.</param>
        /// <param name="className">Its window class.</param>
        /// <param name="title">Its title bar text.</param>
        /// <param name="width">Client area width, in pixels.</param>
        /// <param name="height">Client area height, in pixels.</param>
        public GameWindow(
            IntPtr handle, int processId, string className, string title, int width, int height)
        {
            Handle = handle;
            ProcessId = processId;
            ClassName = className;
            Title = title;
            Width = width;
            Height = height;
        }

        /// <summary>The window handle.</summary>
        public IntPtr Handle { get; }

        /// <summary>The process that owns it.</summary>
        public int ProcessId { get; }

        /// <summary>Its window class, which is how the game is told from its console.</summary>
        public string ClassName { get; }

        /// <summary>Its title bar text.</summary>
        public string Title { get; }

        /// <summary>Client area width, in pixels.</summary>
        public int Width { get; }

        /// <summary>Client area height, in pixels.</summary>
        public int Height { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{ClassName} {Width}x{Height} '{Title}'";
        }
    }

    /// <summary>
    /// Finding and measuring the game's window.
    /// </summary>
    /// <remarks>
    /// <para><c>Process.MainWindowHandle</c> is not good enough for a modded game.
    /// BepInEx opens a console, the process therefore owns at least two top-level
    /// windows, and the console is usually the one MainWindowHandle returns. A console
    /// never changes and is whatever size it was left at, so a screenshot-driven test
    /// pointed at one reports that loading finished immediately at a resolution the game
    /// never used - plausible, and entirely wrong.</para>
    ///
    /// <para>So the windows are enumerated and chosen by class:
    /// <see cref="UnityWindowClass"/> is the renderer,
    /// <see cref="ConsoleWindowClass"/> is never acceptable.</para>
    /// </remarks>
    public static class GameWindows
    {
        /// <summary>The window class of a Unity standalone player.</summary>
        public const string UnityWindowClass = "UnityWndClass";

        /// <summary>The window class of a console. Never the game.</summary>
        public const string ConsoleWindowClass = "ConsoleWindowClass";

        private delegate bool EnumWindowsProc(IntPtr window, IntPtr context);

        [DllImport("user32.dll")]
        private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr context);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetClassNameW(IntPtr window, StringBuilder name, int capacity);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int capacity);

        [DllImport("user32.dll")]
        private static extern bool IsWindowVisible(IntPtr window);

        [DllImport("user32.dll")]
        private static extern bool GetClientRect(IntPtr window, out Rect rect);

        [DllImport("user32.dll")]
        private static extern bool ClientToScreen(IntPtr window, ref Point point);

        [DllImport("user32.dll")]
        private static extern bool SetForegroundWindow(IntPtr window);

        [DllImport("user32.dll")]
        private static extern bool ShowWindow(IntPtr window, int command);

        [DllImport("user32.dll")]
        private static extern IntPtr GetForegroundWindow();

        [StructLayout(LayoutKind.Sequential)]
        private struct Rect
        {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Point
        {
            public int X;
            public int Y;
        }

        private const int SwRestore = 9;

        /// <summary>Every visible top-level window a process owns.</summary>
        /// <param name="processId">The process to enumerate.</param>
        public static GameWindow[] OfProcess(int processId)
        {
            var found = new List<GameWindow>();

            EnumWindows(
                (window, context) =>
                {
                    GetWindowThreadProcessId(window, out uint owner);
                    if (owner == (uint)processId && IsWindowVisible(window))
                    {
                        found.Add(Describe(window, processId));
                    }

                    return true;
                },
                IntPtr.Zero);

            return found.ToArray();
        }

        /// <summary>Describes one window.</summary>
        /// <param name="window">The handle.</param>
        /// <param name="processId">Its owning process.</param>
        public static GameWindow Describe(IntPtr window, int processId)
        {
            var className = new StringBuilder(256);
            GetClassNameW(window, className, className.Capacity);

            var title = new StringBuilder(512);
            GetWindowTextW(window, title, title.Capacity);

            GetClientRect(window, out Rect rect);

            return new GameWindow(
                window,
                processId,
                className.ToString(),
                title.ToString(),
                rect.Right - rect.Left,
                rect.Bottom - rect.Top);
        }

        /// <summary>
        /// The client area in screen coordinates: where to capture from.
        /// </summary>
        /// <remarks>
        /// The client rect rather than the window rect, so a title bar and borders stay
        /// out of a comparison they would only add noise to.
        /// </remarks>
        /// <param name="window">The window.</param>
        /// <param name="x">Screen x of the client area's top-left.</param>
        /// <param name="y">Screen y of the client area's top-left.</param>
        /// <param name="width">Client width.</param>
        /// <param name="height">Client height.</param>
        /// <returns>False if Windows would not answer.</returns>
        public static bool TryGetClientBounds(
            IntPtr window, out int x, out int y, out int width, out int height)
        {
            x = 0;
            y = 0;
            width = 0;
            height = 0;

            if (!GetClientRect(window, out Rect rect))
            {
                return false;
            }

            var origin = default(Point);
            if (!ClientToScreen(window, ref origin))
            {
                return false;
            }

            x = origin.X;
            y = origin.Y;
            width = rect.Right - rect.Left;
            height = rect.Bottom - rect.Top;
            return true;
        }

        /// <summary>
        /// Brings a window to the front and reports whether it actually got there.
        /// </summary>
        /// <remarks>
        /// Reports rather than assumes: Windows refuses foreground changes from a
        /// process that does not currently have it, and both screen capture and input
        /// injection are silently wrong when the wrong window is in front.
        /// </remarks>
        /// <param name="window">The window to raise.</param>
        public static bool BringToFront(IntPtr window)
        {
            ShowWindow(window, SwRestore);
            SetForegroundWindow(window);
            return GetForegroundWindow() == window;
        }

        /// <summary>Whether this window is currently the foreground one.</summary>
        /// <param name="window">The window to test.</param>
        public static bool IsForeground(IntPtr window)
        {
            return GetForegroundWindow() == window;
        }
    }
}
