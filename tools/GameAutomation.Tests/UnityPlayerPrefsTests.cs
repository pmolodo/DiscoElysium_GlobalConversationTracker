// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The PlayerPrefs value-name hash, checked against names the game really wrote.
    /// </summary>
    /// <remarks>
    /// These four came out of HKCU\Software\ZAUM Studio\Disco Elysium on a machine that
    /// had been playing the game. They are the point of the test: a mistyped hash does not
    /// fail, it creates a new value Unity never reads, so the setting silently does
    /// nothing. Only real observed names prove the computation.
    /// </remarks>
    public class UnityPlayerPrefsTests
    {
        [Theory]
        [InlineData("Screenmanager Resolution Width", "Screenmanager Resolution Width_h182942802")]
        [InlineData("Screenmanager Resolution Height", "Screenmanager Resolution Height_h2627697771")]
        [InlineData("Screenmanager Fullscreen mode", "Screenmanager Fullscreen mode_h3630240806")]
        [InlineData(
            "Screenmanager Resolution Use Native",
            "Screenmanager Resolution Use Native_h1405027254")]
        public void ValueNamesMatchWhatTheGameWrote(string key, string expected)
        {
            Assert.Equal(expected, UnityPlayerPrefs.ValueName(key));
        }

        /// <summary>The constants must be the strings the hashes were verified against.</summary>
        [Fact]
        public void TheKeyConstantsAreTheVerifiedNames()
        {
            Assert.Equal(
                "Screenmanager Resolution Width_h182942802",
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.ResolutionWidth));
            Assert.Equal(
                "Screenmanager Resolution Height_h2627697771",
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.ResolutionHeight));
            Assert.Equal(
                "Screenmanager Resolution Use Native_h1405027254",
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.UseNativeResolution));
            Assert.Equal(
                "Screenmanager Fullscreen mode_h3630240806",
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.FullScreenMode));
        }

        /// <summary>
        /// The hash depends only on the name, which is what makes it safe to compute.
        /// </summary>
        [Fact]
        public void TheHashIsStableForAGivenName()
        {
            Assert.Equal(
                UnityPlayerPrefs.ValueName("anything at all"),
                UnityPlayerPrefs.ValueName("anything at all"));
        }

        /// <summary>Names differing by one character must not collide here.</summary>
        [Fact]
        public void DifferentNamesGetDifferentValueNames()
        {
            Assert.NotEqual(
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.ResolutionWidth),
                UnityPlayerPrefs.ValueName(UnityPlayerPrefs.ResolutionHeight));
        }

        /// <summary>
        /// Unity's mode numbering is not the game's, and confusing them is easy: both use
        /// 1, for opposite things. The game's DISPLAY MODE 1 is windowed; Unity's 1 is
        /// borderless fullscreen at the native resolution.
        /// </summary>
        [Fact]
        public void UnityWindowedIsNotTheGamesWindowedValue()
        {
            Assert.Equal(3, UnityPlayerPrefs.Windowed);
            Assert.Equal(1, UnityPlayerPrefs.FullScreenWindow);
            Assert.Equal(1, DisplaySettings.WindowedMode);
            Assert.NotEqual(UnityPlayerPrefs.Windowed, DisplaySettings.WindowedMode);
        }

        [Fact]
        public void ANullKeyIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => UnityPlayerPrefs.ValueName(null!));
        }
    }
}
