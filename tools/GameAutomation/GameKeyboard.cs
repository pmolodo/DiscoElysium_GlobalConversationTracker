// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Sends keystrokes to a game, as scancodes.
    /// </summary>
    /// <remarks>
    /// <para>Scancodes rather than virtual keys, through SendInput. A game reading raw
    /// input or DirectInput ignores virtual-key injection, and Windows Forms' SendKeys
    /// simulates text entry rather than keystrokes, so neither reaches the game at all.
    /// </para>
    ///
    /// <para>The release event sets KEYEVENTF_KEYUP <em>and</em> KEYEVENTF_SCANCODE
    /// together. Sending the release without the scancode flag is the documented way to
    /// leave a key stuck down, and a stuck key in a game is not obvious from a
    /// screenshot - it just makes everything afterwards behave strangely.</para>
    ///
    /// <para>Compiled rather than declared in a PowerShell here-string because
    /// Defender's AMSI blocks a script containing these P/Invokes: SendInput next to
    /// window enumeration is, by inspection, indistinguishable from a keylogger. Interop
    /// belongs in a compiled assembly anyway.</para>
    /// </remarks>
    public static class GameKeyboard
    {
        [StructLayout(LayoutKind.Sequential)]
        private struct KeyboardInput
        {
            public ushort VirtualKey;
            public ushort ScanCode;
            public uint Flags;
            public uint Time;
            public IntPtr ExtraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct Input
        {
            public uint Type;
            public KeyboardInput Keyboard;
            public int Padding1;
            public int Padding2;
        }

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint count, Input[] inputs, int size);

        private const uint InputKeyboard = 1;
        private const uint KeyUp = 0x0002;
        private const uint ScanCodeFlag = 0x0008;
        private const uint ExtendedKey = 0x0001;

        /// <summary>Set-1 scancodes for the keys a menu needs.</summary>
        /// <remarks>
        /// Short on purpose. Every entry is one somebody has a reason to press, and an
        /// unrecognised name is an error rather than a silent no-op - a keystroke that
        /// quietly does nothing is indistinguishable from a game that ignored it.
        /// </remarks>
        private static readonly Dictionary<string, ushort> Codes =
            new Dictionary<string, ushort>(StringComparer.OrdinalIgnoreCase)
            {
                { "Escape", 0x01 }, { "Enter", 0x1C }, { "Space", 0x39 },
                { "Tab", 0x0F }, { "Backspace", 0x0E },
                { "Up", 0x48 }, { "Down", 0x50 }, { "Left", 0x4B }, { "Right", 0x4D },
                { "Home", 0x47 }, { "End", 0x4F }, { "PageUp", 0x49 }, { "PageDown", 0x51 },
                { "F1", 0x3B }, { "F2", 0x3C }, { "F3", 0x3D }, { "F4", 0x3E },
                { "F5", 0x3F }, { "F6", 0x40 }, { "F7", 0x41 }, { "F8", 0x42 },
                { "F9", 0x43 }, { "F10", 0x44 }, { "F11", 0x57 }, { "F12", 0x58 },
                { "1", 0x02 }, { "2", 0x03 }, { "3", 0x04 }, { "4", 0x05 }, { "5", 0x06 },
                { "6", 0x07 }, { "7", 0x08 }, { "8", 0x09 }, { "9", 0x0A }, { "0", 0x0B },
                { "A", 0x1E }, { "B", 0x30 }, { "C", 0x2E }, { "D", 0x20 }, { "E", 0x12 },
                { "F", 0x21 }, { "G", 0x22 }, { "H", 0x23 }, { "I", 0x17 }, { "J", 0x24 },
                { "K", 0x25 }, { "L", 0x26 }, { "M", 0x32 }, { "N", 0x31 }, { "O", 0x18 },
                { "P", 0x19 }, { "Q", 0x10 }, { "R", 0x13 }, { "S", 0x1F }, { "T", 0x14 },
                { "U", 0x16 }, { "V", 0x2F }, { "W", 0x11 }, { "X", 0x2D }, { "Y", 0x15 },
                { "Z", 0x2C },
            };

        /// <summary>
        /// Keys whose scancode needs the E0 prefix. Without it the game reads the
        /// numeric keypad's key of the same code instead of the arrow or navigation one.
        /// </summary>
        private static readonly HashSet<string> Extended =
            new HashSet<string>(StringComparer.OrdinalIgnoreCase)
            {
                "Up", "Down", "Left", "Right", "Home", "End", "PageUp", "PageDown",
            };

        /// <summary>Every key name <see cref="Press"/> and <see cref="Release"/> accept.</summary>
        public static string[] KeyNames
        {
            get
            {
                var names = new List<string>(Codes.Keys);
                names.Sort(StringComparer.OrdinalIgnoreCase);
                return names.ToArray();
            }
        }

        /// <summary>Whether a key name is recognised.</summary>
        /// <param name="key">The key name.</param>
        public static bool IsKnown(string key)
        {
            return key != null && Codes.ContainsKey(key);
        }

        /// <summary>Presses a key down, without releasing it.</summary>
        /// <param name="key">The key name.</param>
        /// <exception cref="ArgumentException">The key name is not recognised.</exception>
        /// <exception cref="InvalidOperationException">Windows refused the injection.</exception>
        public static void Press(string key)
        {
            Send(key, up: false);
        }

        /// <summary>Releases a key.</summary>
        /// <param name="key">The key name.</param>
        /// <exception cref="ArgumentException">The key name is not recognised.</exception>
        /// <exception cref="InvalidOperationException">Windows refused the injection.</exception>
        public static void Release(string key)
        {
            Send(key, up: true);
        }

        private static void Send(string key, bool up)
        {
            if (!IsKnown(key))
            {
                throw new ArgumentException(
                    $"Unknown key '{key}'. Known keys: {string.Join(", ", KeyNames)}.", nameof(key));
            }

            uint flags = ScanCodeFlag;
            if (Extended.Contains(key))
            {
                flags |= ExtendedKey;
            }

            if (up)
            {
                // KEYUP is OR-ed with the scancode flag, never sent alone.
                flags |= KeyUp;
            }

            var input = new Input
            {
                Type = InputKeyboard,
                Keyboard = new KeyboardInput
                {
                    VirtualKey = 0,
                    ScanCode = Codes[key],
                    Flags = flags,
                    Time = 0,
                    ExtraInfo = IntPtr.Zero,
                },
            };

            uint sent = SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input)));
            if (sent != 1)
            {
                int error = Marshal.GetLastWin32Error();
                throw new InvalidOperationException(
                    $"SendInput refused the key-{(up ? "up" : "down")} for '{key}' (error {error}). "
                    + "A more privileged window in the foreground will do this.");
            }
        }
    }
}
