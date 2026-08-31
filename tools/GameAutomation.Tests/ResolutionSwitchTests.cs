// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// When waiting for the game to apply its resolution proves something, and when it
    /// only appears to.
    /// </summary>
    public class ResolutionSwitchTests
    {
        private static GameWindow Window(int width, int height)
        {
            return new GameWindow(
                IntPtr.Zero, 0, GameWindows.UnityWindowClass, "Disco Elysium", width, height);
        }

        private static readonly DisplaySettings TestResolution =
            new DisplaySettings(1280, 720, DisplaySettings.WindowedMode);

        /// <summary>The normal case: opens at the desktop size, switches down.</summary>
        [Fact]
        public void AWindowOpeningLargerCanBeSeenToSwitch()
        {
            Assert.True(GameSession.CanObserveResolutionSwitch(
                Window(3840, 1200), TestResolution));
        }

        /// <summary>
        /// The case that makes the check a lie. A player already running at the test
        /// resolution gets a window that opens correct, so the wait returns on its first
        /// poll - before the game has read anything. Passing then says only "the size is
        /// right", never "the settings were applied", and a game ignoring the settings
        /// file entirely would look exactly the same from outside.
        /// </summary>
        [Fact]
        public void AWindowOpeningAtTheWantedSizeCannotShowASwitch()
        {
            Assert.False(GameSession.CanObserveResolutionSwitch(
                Window(1280, 720), TestResolution));
        }

        /// <summary>One matching dimension is not a match.</summary>
        [Theory]
        [InlineData(1280, 800)]
        [InlineData(1920, 720)]
        public void MatchingOnlyOneDimensionStillLeavesASwitchToSee(int width, int height)
        {
            Assert.True(GameSession.CanObserveResolutionSwitch(
                Window(width, height), TestResolution));
        }

        [Fact]
        public void NullArgumentsAreRefused()
        {
            Assert.Throws<ArgumentNullException>(
                () => GameSession.CanObserveResolutionSwitch(null!, TestResolution));
            Assert.Throws<ArgumentNullException>(
                () => GameSession.CanObserveResolutionSwitch(Window(800, 600), null!));
        }
    }
}
