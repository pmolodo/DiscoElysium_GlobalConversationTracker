// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// A probe command threw, so the event the run was waiting for will never come.
    /// </summary>
    /// <remarks>
    /// Distinct from a timeout because it means something different and wants a different
    /// fix. A timeout says the game is slow, or stuck, or that the harness asked for the
    /// wrong thing; this says the game answered, and the answer was a refusal - with a
    /// reason attached. Reporting it as a timeout throws that reason away and points the
    /// reader at the game's speed instead of at what it said.
    /// </remarks>
    public sealed class ProbeCommandFailedException : Exception
    {
        /// <summary>Records a failed command.</summary>
        /// <param name="command">The command that threw, if the probe named it.</param>
        /// <param name="message">Why it threw, if the probe said.</param>
        /// <param name="what">What the run was waiting for when it happened.</param>
        public ProbeCommandFailedException(string? command, string? message, string? what)
            : base(
                $"The probe could not carry out '{command ?? "a command"}' while waiting for "
                + $"{what ?? "an event"}: {message ?? "it did not say why"}.")
        {
            Command = command;
            Reason = message;
        }

        /// <summary>The command that threw, if the probe named it.</summary>
        public string? Command { get; }

        /// <summary>Why it threw, if the probe said.</summary>
        public string? Reason { get; }
    }

    /// <summary>
    /// The game went away while the run was waiting on it.
    /// </summary>
    /// <remarks>
    /// Also distinct from a timeout: nothing is going to arrive, so the only useful thing
    /// left is to stop and let the caller put the profile back.
    /// </remarks>
    public sealed class ProbeGoneException : Exception
    {
        /// <summary>Records that the game is gone.</summary>
        /// <param name="message">What was being waited for, and how it was noticed.</param>
        public ProbeGoneException(string message)
            : base(message)
        {
        }
    }
}
