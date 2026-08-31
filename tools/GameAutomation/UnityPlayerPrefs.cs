// SPDX-License-Identifier: MIT
using System;
using System.Globalization;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Unity's own screen settings, which are what actually size the window.
    /// </summary>
    /// <remarks>
    /// <para>Separate from the game's Settings.json, and it wins. Unity creates the window
    /// from PlayerPrefs before a line of game code runs, and "Screenmanager Resolution Use
    /// Native" being 1 means it uses the display's resolution and ignores the stored width
    /// and height entirely. That is why editing only the width and height changes nothing,
    /// and why staging a settings file asking for 1280x720 produced a 3840x1200 window
    /// that never switched.</para>
    ///
    /// <para>On Windows these live in the registry under HKCU\Software\[company]\[product],
    /// with each key name suffixed by "_h" and a hash. The hash is a DJB2 variant over the
    /// key name alone, so it is stable and computable rather than something to copy by
    /// hand - which is worth doing in code, because a mistyped hash creates a NEW key that
    /// Unity ignores, and silently does nothing at all.</para>
    /// </remarks>
    public static class UnityPlayerPrefs
    {
        /// <summary>The registry key holding this game's PlayerPrefs.</summary>
        public const string DiscoElysiumKey = @"HKCU\Software\ZAUM Studio\Disco Elysium";

        /// <summary>Width of the window Unity creates.</summary>
        public const string ResolutionWidth = "Screenmanager Resolution Width";

        /// <summary>Height of the window Unity creates.</summary>
        public const string ResolutionHeight = "Screenmanager Resolution Height";

        /// <summary>
        /// When 1, Unity uses the display's resolution and ignores the stored size.
        /// </summary>
        public const string UseNativeResolution = "Screenmanager Resolution Use Native";

        /// <summary>Unity's FullScreenMode, which is NOT the game's own DISPLAY MODE.</summary>
        /// <remarks>
        /// Two different numbering schemes, and both use 1. In the game's Settings.json,
        /// DISPLAY MODE 1 means windowed. Here, 1 is FullScreenWindow - borderless
        /// fullscreen, documented as using the native display resolution. Windowed is 3.
        /// </remarks>
        public const string FullScreenMode = "Screenmanager Fullscreen mode";

        /// <summary>Unity's FullScreenMode.ExclusiveFullScreen.</summary>
        public const int ExclusiveFullScreen = 0;

        /// <summary>Unity's FullScreenMode.FullScreenWindow: borderless, native size.</summary>
        public const int FullScreenWindow = 1;

        /// <summary>Unity's FullScreenMode.MaximizedWindow.</summary>
        public const int MaximizedWindow = 2;

        /// <summary>Unity's FullScreenMode.Windowed: a real window at the stored size.</summary>
        public const int Windowed = 3;

        /// <summary>The registry value name Unity stores a key under.</summary>
        /// <remarks>
        /// DJB2 over the key name: hash starts at 5381, then for each character
        /// hash = hash * 33 XOR character. Depends on nothing but the name, so it is
        /// stable for the life of the game. Verified against the names this game has
        /// actually written.
        /// </remarks>
        /// <param name="key">The PlayerPrefs key name.</param>
        /// <exception cref="ArgumentNullException"><paramref name="key"/> is null.</exception>
        public static string ValueName(string key)
        {
            if (key == null)
            {
                throw new ArgumentNullException(nameof(key));
            }

            unchecked
            {
                uint hash = 5381;
                foreach (char c in key)
                {
                    hash = (hash * 33) ^ c;
                }

                return key + "_h" + hash.ToString(CultureInfo.InvariantCulture);
            }
        }
    }
}
