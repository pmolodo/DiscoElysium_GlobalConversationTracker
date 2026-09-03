// SPDX-License-Identifier: MIT
using System;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Getting the game in front, a bounded number of times, and admitting when it
    /// cannot.
    /// </summary>
    /// <remarks>
    /// <para>Both things a harness does to a game - reading its screen and sending it
    /// keys - are silently wrong when another window is in front. A capture reads screen
    /// pixels, so it photographs whatever is covering the game; a keypress goes to the
    /// foreground window, so it is typed into whatever that is. Neither fails. Both
    /// produce a plausible-looking result about the wrong window.</para>
    ///
    /// <para>Bounded because Windows is allowed to refuse a foreground change, and
    /// something may be deliberately holding focus. Retrying forever turns that into a
    /// fight nothing wins and a log that is one line repeated. The count resets when the
    /// caller says the screen changed, so a later screen gets its own attempts.</para>
    ///
    /// <para>Bounded, but never permanently. Giving up for good deadlocks a caller whose
    /// only way to see the next screen is to be in front of it: during the game's first
    /// seconds no raise succeeds, and a run that spent its four attempts there and stopped
    /// would sit out its whole timeout against a game that came forward at second ten. So
    /// the allowance comes back after <see cref="RetryAfter"/>, which keeps four attempts
    /// per window rather than four per run.</para>
    ///
    /// <para>The three system calls are injected so the decisions can be tested without
    /// a game, a desktop, or a race.</para>
    /// </remarks>
    public sealed class ForegroundRaiser
    {
        /// <summary>How many times to try raising the game per screen.</summary>
        public const int DefaultMaxAttempts = 4;

        /// <summary>How long to give a raise before believing it did not work.</summary>
        public static readonly TimeSpan DefaultSettle = TimeSpan.FromMilliseconds(200);

        /// <summary>How long a spent allowance takes to come back.</summary>
        public static readonly TimeSpan DefaultRetryAfter = TimeSpan.FromSeconds(5);

        private readonly Func<IntPtr, bool> _isForeground;
        private readonly Func<IntPtr, bool> _bringToFront;
        private readonly Action<TimeSpan> _wait;
        private readonly Func<DateTime> _now;
        private readonly TimeSpan _settle;

        private int _attempts;
        private DateTime _gaveUpAt;

        /// <summary>Creates a raiser that drives the real windows.</summary>
        /// <param name="maxAttempts">How many raises to try per screen.</param>
        /// <param name="settle">How long to give a raise, or null for the default.</param>
        public ForegroundRaiser(int maxAttempts = DefaultMaxAttempts, TimeSpan? settle = null)
            : this(
                GameWindows.IsForeground,
                GameWindows.BringToFront,
                Thread.Sleep,
                maxAttempts,
                settle,
                () => DateTime.UtcNow)
        {
        }

        /// <summary>Creates a raiser over injected system calls. For tests.</summary>
        /// <param name="isForeground">Whether a window is in front.</param>
        /// <param name="bringToFront">Asks for a window to be raised.</param>
        /// <param name="wait">Waits for a raise to take effect.</param>
        /// <param name="maxAttempts">How many raises to try per screen.</param>
        /// <param name="settle">How long to give a raise, or null for the default.</param>
        /// <param name="now">Reads the clock, or null for the real one.</param>
        /// <param name="retryAfter">
        /// How long a spent allowance takes to come back, or null for the default.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="maxAttempts"/> is negative.
        /// </exception>
        public ForegroundRaiser(
            Func<IntPtr, bool> isForeground,
            Func<IntPtr, bool> bringToFront,
            Action<TimeSpan> wait,
            int maxAttempts = DefaultMaxAttempts,
            TimeSpan? settle = null,
            Func<DateTime>? now = null,
            TimeSpan? retryAfter = null)
        {
            _isForeground = isForeground ?? throw new ArgumentNullException(nameof(isForeground));
            _bringToFront = bringToFront ?? throw new ArgumentNullException(nameof(bringToFront));
            _wait = wait ?? throw new ArgumentNullException(nameof(wait));
            if (maxAttempts < 0)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(maxAttempts), maxAttempts, "Cannot try a negative number of times.");
            }

            MaxAttempts = maxAttempts;
            _settle = settle ?? DefaultSettle;
            _now = now ?? (() => DateTime.UtcNow);
            RetryAfter = retryAfter ?? DefaultRetryAfter;
        }

        /// <summary>How long a spent allowance takes to come back.</summary>
        public TimeSpan RetryAfter { get; }

        /// <summary>How many raises are allowed per screen.</summary>
        public int MaxAttempts { get; }

        /// <summary>How many have been spent on the current screen.</summary>
        public int Attempts => _attempts;

        /// <summary>Whether this screen's attempts are used up.</summary>
        public bool GaveUp => _attempts >= MaxAttempts;

        /// <summary>Gives the next screen its own attempts.</summary>
        public void Reset()
        {
            _attempts = 0;
        }

        /// <summary>
        /// Makes sure the window is in front, and says whether it is.
        /// </summary>
        /// <remarks>
        /// A window already in front costs nothing and spends no attempt: the common case
        /// is not a retry. The return value is the point - a caller that ignores it and
        /// acts anyway is back to reading and typing into somebody else's window.
        /// </remarks>
        /// <param name="window">The window to raise.</param>
        /// <returns>Whether the window is in front now.</returns>
        public bool Ensure(IntPtr window)
        {
            if (_isForeground(window))
            {
                return true;
            }

            if (GaveUp)
            {
                if (_now() - _gaveUpAt < RetryAfter)
                {
                    return false;
                }

                Reset();

                // An allowance of none is still none once it has come back. Without this
                // the cooldown would hand a raise to a caller that asked for no raising.
                if (GaveUp)
                {
                    return false;
                }
            }

            _attempts++;
            if (_attempts >= MaxAttempts)
            {
                _gaveUpAt = _now();
            }

            _bringToFront(window);
            _wait(_settle);
            return _isForeground(window);
        }

        /// <summary>How to describe the current state, for a log line.</summary>
        /// <remarks>
        /// Said once per state rather than per attempt: at two polls a second the same
        /// sentence repeated is what buries the run's real output.
        /// </remarks>
        public string Describe() =>
            GaveUp
                ? $"not in front, and raising it failed {MaxAttempts} times; trying again in "
                    + $"{RetryAfter.TotalSeconds:N0}s - bring it forward yourself if it is not "
                    + "coming"
                : $"not in front - raising it ({_attempts} of {MaxAttempts})";
    }
}
