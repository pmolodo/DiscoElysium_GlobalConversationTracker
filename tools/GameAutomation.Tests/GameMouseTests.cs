// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Mapping screen coordinates onto SendInput's absolute axis.
    /// </summary>
    /// <remarks>
    /// Only the arithmetic is covered here. Moving a real cursor and clicking needs a
    /// desktop nobody else is using, which is what <see cref="InGameFactAttribute"/>
    /// exists to gate; getting the mapping wrong is the failure that would put every
    /// click on the wrong monitor, and it can be checked without one.
    /// </remarks>
    public class GameMouseTests
    {
        private const int Full = 65535;

        [Fact]
        public void TheEndsOfAnAxisMapToTheEndsOfTheRange()
        {
            Assert.Equal(0, GameMouse.ToAbsolute(0, 0, 1920));
            Assert.Equal(Full, GameMouse.ToAbsolute(1919, 0, 1920));
        }

        [Fact]
        public void TheMiddleOfAnAxisMapsToTheMiddleOfTheRange()
        {
            // Within one step, never exactly: the range is 0..65535, an odd number of
            // steps, so the true midpoint of any axis falls half a step off a whole one.
            Assert.InRange(GameMouse.ToAbsolute(960, 0, 1921), Full / 2, (Full / 2) + 1);
        }

        [Fact]
        public void ANegativeOriginIsSubtractedRatherThanClamped()
        {
            // A second monitor left of the primary one puts the virtual desktop's origin
            // at a negative x. Ignoring it would map every point on that monitor to 0,
            // which is the whole-desktop-left edge rather than where it was asked for.
            Assert.Equal(0, GameMouse.ToAbsolute(-1920, -1920, 3840));
            Assert.Equal(Full, GameMouse.ToAbsolute(1919, -1920, 3840));
            Assert.InRange(
                GameMouse.ToAbsolute(-1, -1920, 3839), Full / 2, (Full / 2) + 1);
        }

        [Fact]
        public void APointOutsideTheDesktopIsClampedRatherThanWrapped()
        {
            // Left as a clamp rather than an exception: the caller cannot always know the
            // desktop's bounds, and MoveTo reads the cursor back afterwards, so an
            // impossible point is reported by the check that matters rather than here.
            Assert.Equal(0, GameMouse.ToAbsolute(-5000, 0, 1920));
            Assert.Equal(Full, GameMouse.ToAbsolute(5000, 0, 1920));
        }

        [Fact]
        public void MappingIsMonotonic()
        {
            int previous = -1;
            for (int x = 0; x < 1920; x++)
            {
                int absolute = GameMouse.ToAbsolute(x, 0, 1920);
                Assert.True(absolute >= previous, $"x={x} went backwards");
                previous = absolute;
            }
        }

        [Fact]
        public void RoundingIsNotBiasedTowardsTheLeft()
        {
            // Truncating instead of rounding costs up to a pixel on every click, always
            // in the same direction, which is invisible until something small is aimed at.
            // Each pixel must map back onto itself.
            const int width = 1920;
            for (int x = 0; x < width; x++)
            {
                int absolute = GameMouse.ToAbsolute(x, 0, width);
                int back = (int)Math.Round((double)absolute * (width - 1) / Full);
                Assert.Equal(x, back);
            }
        }

        [Fact]
        public void AnEmptyDesktopIsRefused()
        {
            Assert.Throws<ArgumentOutOfRangeException>(() => GameMouse.ToAbsolute(0, 0, 0));
            Assert.Throws<ArgumentOutOfRangeException>(() => GameMouse.ToAbsolute(0, 0, -1));
        }

        [Fact]
        public void ASingleColumnDesktopDoesNotDivideByZero()
        {
            Assert.Equal(0, GameMouse.ToAbsolute(0, 0, 1));
        }

        [Fact]
        public void AnUnknownButtonIsRefusedRatherThanIgnored()
        {
            Assert.Throws<ArgumentException>(() => GameMouse.Click("Middle"));
        }

        [Fact]
        public void ClickingOutsideTheWindowIsRefused()
        {
            var window = new GameWindow(
                IntPtr.Zero, 0, GameWindows.UnityWindowClass, "disco", 1280, 720);

            Assert.Throws<ArgumentOutOfRangeException>(
                () => GameMouse.ClickInWindow(window, 1280, 360));
            Assert.Throws<ArgumentOutOfRangeException>(
                () => GameMouse.ClickInWindow(window, 640, 720));
            Assert.Throws<ArgumentOutOfRangeException>(
                () => GameMouse.ClickInWindow(window, -1, 360));
        }

        [Fact]
        public void ClickingInNoWindowIsRefused()
        {
            Assert.Throws<ArgumentNullException>(
                () => GameMouse.ClickInWindow(null!, 0, 0));
        }
    }
}
