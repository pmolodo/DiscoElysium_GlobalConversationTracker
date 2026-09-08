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
    /// A command nobody picked up was still sitting there when the next one was written.
    /// </summary>
    /// <remarks>
    /// <para>An InvalidOperationException, which is what this was before it had a name,
    /// so nothing that catches the base type notices the change. It is worth a type of
    /// its own because the thing that can explain it is somewhere else: the probe has
    /// gone quiet, and only the caller that LAUNCHED the game knows whether the game is
    /// still there and how it ended. A named exception lets that caller catch this one
    /// case and add what it knows, without wrapping every command it sends.</para>
    /// </remarks>
    public sealed class ProbePendingException : InvalidOperationException
    {
        /// <summary>Records a command that was never picked up.</summary>
        /// <param name="message">Which command, and where.</param>
        public ProbePendingException(string message)
            : base(message)
        {
        }

        /// <summary>Records the same thing with more known about it.</summary>
        /// <param name="message">Which command, where, and how the game ended.</param>
        /// <param name="cause">The refusal this adds to.</param>
        public ProbePendingException(string message, Exception? cause)
            : base(message, cause)
        {
        }
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
