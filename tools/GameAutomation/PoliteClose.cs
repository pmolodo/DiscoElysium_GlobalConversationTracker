// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;

namespace GlobalConversationTracker.Automation
{
    /// <summary>What happened when a process was asked to close.</summary>
    public sealed class CloseAttempt
    {
        /// <summary>Creates a result.</summary>
        /// <param name="processId">The process asked.</param>
        /// <param name="name">Its name.</param>
        /// <param name="closed">Whether it had exited by the deadline.</param>
        /// <param name="detail">What happened, for a person to read.</param>
        public CloseAttempt(int processId, string name, bool closed, string detail)
        {
            ProcessId = processId;
            Name = name;
            Closed = closed;
            Detail = detail;
        }

        /// <summary>The process asked.</summary>
        public int ProcessId { get; }

        /// <summary>Its name.</summary>
        public string Name { get; }

        /// <summary>Whether it had exited by the deadline.</summary>
        public bool Closed { get; }

        /// <summary>What happened.</summary>
        public string Detail { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Name} (pid {ProcessId}): {Detail}";
        }
    }

    /// <summary>
    /// Asking a process to close, and taking no for an answer.
    /// </summary>
    /// <remarks>
    /// <para>For the case an Explorer window cannot cover: an editor with the profile
    /// folder open holds it, and unlike a shell window there is no way to move it off the
    /// folder without closing it.</para>
    ///
    /// <para>The request is <c>CloseMainWindow</c>, which posts WM_CLOSE - the same thing
    /// clicking the X does. An editor with unsaved changes answers it by putting up a save
    /// prompt and STAYING OPEN, which is the correct behaviour and must be treated as a
    /// refusal rather than something to push past.</para>
    ///
    /// <para>So there is a deadline, and nothing is ever killed. If the process is still
    /// running when it expires - a save prompt waiting for somebody who is not at the
    /// keyboard, or work they mean to keep - it is left alone and reported. Losing
    /// somebody's unsaved work to speed up a test would be a bad trade at any timeout.
    /// </para>
    /// </remarks>
    public static class PoliteClose
    {
        /// <summary>How long to give a process to go, before leaving it alone.</summary>
        /// <remarks>
        /// The wait is for a PERSON, not a process: an editor with unsaved changes puts
        /// up a save prompt and stays open, so this has to be long enough to notice that
        /// and answer it. Thirty seconds. Short enough that an unattended run is not held
        /// up indefinitely by a prompt nobody is going to see.
        /// </remarks>
        public static readonly TimeSpan DefaultDeadline = TimeSpan.FromSeconds(30);

        /// <summary>Asks the named processes among a set of holders to close.</summary>
        /// <param name="holders">Who is holding the folder.</param>
        /// <param name="askable">
        /// Process names that may be asked, matched case-insensitively as a prefix. Only
        /// these are asked; anything else holding the folder is reported and left.
        /// </param>
        /// <param name="deadline">How long to wait for each, or null for the default.</param>
        /// <param name="announce">Called as each is asked.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static CloseAttempt[] AskToClose(
            IEnumerable<LockHolder> holders,
            IEnumerable<string> askable,
            TimeSpan? deadline = null,
            Action<string>? announce = null)
        {
            if (holders == null)
            {
                throw new ArgumentNullException(nameof(holders));
            }

            if (askable == null)
            {
                throw new ArgumentNullException(nameof(askable));
            }

            var names = new List<string>(askable);
            TimeSpan limit = deadline ?? DefaultDeadline;
            var attempts = new List<CloseAttempt>();

            foreach (LockHolder holder in holders)
            {
                if (!IsAskable(holder.Name, names))
                {
                    continue;
                }

                attempts.Add(Ask(holder, limit, announce));
            }

            return attempts.ToArray();
        }

        /// <summary>Whether a process name is one this is allowed to ask.</summary>
        /// <param name="name">The process name.</param>
        /// <param name="askable">The permitted names, matched as prefixes.</param>
        public static bool IsAskable(string name, IEnumerable<string> askable)
        {
            if (name == null || askable == null)
            {
                return false;
            }

            string bare = name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)
                ? name.Substring(0, name.Length - 4)
                : name;

            foreach (string candidate in askable)
            {
                if (bare.StartsWith(candidate, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }

            return false;
        }

        private static CloseAttempt Ask(LockHolder holder, TimeSpan deadline, Action<string>? announce)
        {
            Process process;
            try
            {
                process = Process.GetProcessById(holder.ProcessId);
            }
            catch (Exception)
            {
                return new CloseAttempt(
                    holder.ProcessId, holder.Name, true, "already gone");
            }

            using (process)
            {
                announce?.Invoke(
                    $"asking {holder.Name} (pid {holder.ProcessId}) to close, waiting up to "
                    + $"{deadline.TotalSeconds:N0}s...");

                bool asked;
                try
                {
                    // Posts WM_CLOSE. A process with no main window - a background helper -
                    // cannot be asked this way and returns false.
                    asked = process.CloseMainWindow();
                }
                catch (Exception error)
                {
                    return new CloseAttempt(
                        holder.ProcessId, holder.Name, false, $"could not be asked: {error.Message}");
                }

                if (!asked)
                {
                    return new CloseAttempt(
                        holder.ProcessId,
                        holder.Name,
                        false,
                        "has no window to close - a background process, so it was left alone");
                }

                if (process.WaitForExit((int)deadline.TotalMilliseconds))
                {
                    return new CloseAttempt(holder.ProcessId, holder.Name, true, "closed");
                }

                // Still running. Almost always a save prompt waiting for somebody. It is
                // NOT killed: the folder staying locked is a smaller loss than the work.
                return new CloseAttempt(
                    holder.ProcessId,
                    holder.Name,
                    false,
                    $"still open after {deadline.TotalSeconds:N0}s - probably asking about "
                        + "unsaved work. Left running; close it yourself if the changes can go");
            }
        }
    }
}
