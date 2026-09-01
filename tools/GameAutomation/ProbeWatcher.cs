// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Waits for the in-game probe to report something, by watching the BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>This is what replaces waiting on the screen. Every screen in this game
    /// animates, so there is nothing to settle on, and the states a run needs to
    /// distinguish - the world is up, the save that just loaded is the right one, the
    /// response menu is drawn - look alike to any pixel threshold loose enough to be
    /// stable. The probe says them outright.</para>
    ///
    /// <para>The whole log is re-read on each poll rather than tailed. It is small, the
    /// poll interval is half a second, and a tail would have to cope with BepInEx
    /// rewriting the file per run and with a partial line at the end - complexity bought
    /// for nothing. Events already seen when the watcher was created are skipped, so a
    /// wait cannot be satisfied by a previous scenario's event: that mistake would make
    /// a test pass by looking at the wrong menu.</para>
    /// </remarks>
    public sealed class ProbeWatcher
    {
        /// <summary>How often the log is re-read.</summary>
        public static readonly TimeSpan DefaultPollInterval = TimeSpan.FromMilliseconds(500);

        private readonly Func<ProbeEvent[]> _read;
        private readonly TimeSpan _poll;
        private int _consumed;

        /// <summary>Watches a BepInEx log file.</summary>
        /// <param name="logPath">The log.</param>
        /// <param name="poll">How often to re-read it, or null for the default.</param>
        /// <exception cref="ArgumentNullException"><paramref name="logPath"/> is null.</exception>
        public ProbeWatcher(string logPath, TimeSpan? poll = null)
            : this(
                () => ProbeLog.ReadFile(
                    logPath ?? throw new ArgumentNullException(nameof(logPath))),
                poll)
        {
            if (logPath == null)
            {
                throw new ArgumentNullException(nameof(logPath));
            }
        }

        /// <summary>Watches whatever a reader returns. For tests.</summary>
        /// <param name="read">Returns every probe event so far, in order.</param>
        /// <param name="poll">How often to call it, or null for the default.</param>
        /// <exception cref="ArgumentNullException"><paramref name="read"/> is null.</exception>
        public ProbeWatcher(Func<ProbeEvent[]> read, TimeSpan? poll = null)
        {
            _read = read ?? throw new ArgumentNullException(nameof(read));
            _poll = poll ?? DefaultPollInterval;
        }

        /// <summary>
        /// Ignores everything the log already holds, so the next wait can only be
        /// satisfied by something that happens from now on.
        /// </summary>
        /// <remarks>
        /// Called before each step of a run - before sending the keys that load a save,
        /// before the click that opens a conversation. Without it a wait would be
        /// answered instantly by the previous scenario's event, and the test would
        /// report on a menu it never caused.
        /// </remarks>
        /// <returns>How many events were skipped.</returns>
        public int Mark()
        {
            _consumed = _read().Length;
            return _consumed;
        }

        /// <summary>Every event since the last <see cref="Mark"/>.</summary>
        public ProbeEvent[] Since()
        {
            ProbeEvent[] all = _read();
            if (all.Length <= _consumed)
            {
                return Array.Empty<ProbeEvent>();
            }

            var fresh = new ProbeEvent[all.Length - _consumed];
            Array.Copy(all, _consumed, fresh, 0, fresh.Length);
            return fresh;
        }

        /// <summary>Waits for an event to match, and consumes everything up to it.</summary>
        /// <param name="matches">What is being waited for.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="what">What to call it in the timeout message.</param>
        /// <param name="progress">Called with each poll, for verbose output.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="TimeoutException">It never happened.</exception>
        public ProbeEvent WaitFor(
            Func<ProbeEvent, bool> matches,
            TimeSpan timeout,
            string what,
            Action<string>? progress = null)
        {
            if (matches == null)
            {
                throw new ArgumentNullException(nameof(matches));
            }

            var clock = Stopwatch.StartNew();
            var seen = new List<string>();
            int lastReport = -1;

            while (true)
            {
                ProbeEvent[] all = _read();
                for (int i = _consumed; i < all.Length; i++)
                {
                    seen.Add(all[i].Name);
                    if (matches(all[i]))
                    {
                        _consumed = i + 1;
                        progress?.Invoke($"saw {what} after {clock.Elapsed.TotalSeconds:N1}s");
                        return all[i];
                    }
                }

                _consumed = all.Length;

                if (clock.Elapsed >= timeout)
                {
                    // Naming what did arrive is the difference between "the game hung"
                    // and "the game did everything except the one thing expected", which
                    // want completely different fixes.
                    string detail = seen.Count == 0
                        ? "The probe reported nothing at all; check that it is installed "
                            + "and that the game loaded it."
                        : $"The probe reported: {string.Join(", ", seen)}.";
                    throw new TimeoutException(
                        $"Waited {timeout.TotalSeconds:N0}s for {what} and it never came. {detail}");
                }

                // Every ten seconds, not every poll: at two polls a second a three-minute
                // wait buries the run's actual output under three hundred identical
                // lines, and the one thing worth saying - that it is still waiting, and
                // for how long - is said just as well by six of them.
                int decisecond = (int)(clock.Elapsed.TotalSeconds / 10);
                if (decisecond != lastReport)
                {
                    lastReport = decisecond;
                    progress?.Invoke($"waiting for {what} ({clock.Elapsed.TotalSeconds:N0}s)");
                }

                Thread.Sleep(_poll);
            }
        }

        /// <summary>Waits for an event of a given name.</summary>
        /// <param name="name">The event name.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="progress">Called with each poll, for verbose output.</param>
        /// <exception cref="TimeoutException">It never happened.</exception>
        public ProbeEvent WaitForEvent(
            string name, TimeSpan timeout, Action<string>? progress = null)
        {
            return WaitFor(
                e => string.Equals(e.Name, name, StringComparison.Ordinal),
                timeout,
                $"a '{name}' event",
                progress);
        }

        /// <summary>Waits for a response menu belonging to one conversation.</summary>
        /// <param name="conversationId">The conversation it must belong to.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="progress">Called with each poll, for verbose output.</param>
        /// <exception cref="TimeoutException">It never happened.</exception>
        public ProbeEvent WaitForMenu(
            int conversationId, TimeSpan timeout, Action<string>? progress = null)
        {
            return WaitFor(
                e => e.Name == "menu" && e.Number("conversation") == conversationId,
                timeout,
                $"a response menu in conversation {conversationId}",
                progress);
        }
    }
}
