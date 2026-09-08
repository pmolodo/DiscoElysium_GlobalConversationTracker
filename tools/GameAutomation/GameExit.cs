// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Globalization;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// How a process the run launched ended, said in words rather than in a number.
    /// </summary>
    /// <remarks>
    /// <para>THE DIAGNOSES ARE OPPOSITE AND THE NUMBER SEPARATES THEM. A run that finds
    /// the game gone has two completely different stories available: it closed - which
    /// means something asked it to, and the question is what - or it crashed, which is
    /// the game's own fault and wants a Player.log rather than a look at the harness.
    /// The exit code says which, and until this existed the harness threw it away and
    /// reported only that the process was no longer there.</para>
    ///
    /// <para>WHY THE NAMES. Windows reports a crash as the structured-exception code,
    /// which arrives as a large negative int - -1073741819 for the access violation a
    /// NullReferenceException in the game's own code produces. Nobody recognises that in
    /// decimal, and looking it up is a detour taken while the run is still fresh in mind
    /// or not at all, so the few that a game actually dies of are named here.</para>
    /// </remarks>
    public static class GameExit
    {
        /// <summary>What an ordinary close leaves behind.</summary>
        public const int Clean = 0;

        /// <summary>What is said when there is no process to ask.</summary>
        public const string Unknown = "and nothing can be learned about how it ended";

        /// <summary>Describes how a process ended, or that it has not.</summary>
        /// <remarks>
        /// A clause rather than a sentence, so it can be joined to whatever noticed:
        /// "the game is no longer running: it exited with code 0 ...". Never throws -
        /// this is only ever called to explain a failure, and a failure to explain a
        /// failure is the worst of both.
        /// </remarks>
        /// <param name="process">The process the run launched, or null.</param>
        public static string Describe(Process? process)
        {
            if (process == null)
            {
                return Unknown;
            }

            try
            {
                if (!process.HasExited)
                {
                    // Worth saying rather than treating as impossible: the caller asks
                    // because SOMETHING went wrong, and "the process is still there" is a
                    // real answer that rules out half the candidates - a stalled game
                    // rather than a dead one.
                    return "it is still running";
                }

                return $"it exited with code {Explain(process.ExitCode)}";
            }
            catch (InvalidOperationException)
            {
                // No process was ever started, or the handle is gone. Guessing it died
                // would be inventing the evidence this exists to supply.
                return Unknown;
            }
            catch (SystemException)
            {
                return Unknown;
            }
        }

        /// <summary>Names an exit code, when the name is worth more than the number.</summary>
        /// <remarks>
        /// The number always comes first and in decimal, because that is what any other
        /// tool looking at the same process will print. The rest is added to it.
        /// </remarks>
        /// <param name="code">The process's exit code.</param>
        public static string Explain(int code)
        {
            string number = code.ToString(CultureInfo.InvariantCulture);
            if (code == Clean)
            {
                return $"{number}, which is an ordinary close rather than a crash";
            }

            string? named = NameOf(code);
            string hex = "0x"
                + unchecked((uint)code).ToString("X8", CultureInfo.InvariantCulture);
            return named == null
                ? $"{number} ({hex}), which is not an ordinary close"
                : $"{number} ({hex}), {named}";
        }

        /// <summary>
        /// What a structured-exception exit code means, if it is one worth naming.
        /// </summary>
        private static string? NameOf(int code)
        {
            switch (unchecked((uint)code))
            {
                case 0xC0000005:
                    // The one a Unity game dies of. A NullReferenceException that escapes
                    // into native code lands here, which is why it is worth naming: the
                    // Player.log's last managed exception is then the lead, not noise.
                    return "an access violation - the game crashed";
                case 0xC0000374:
                    return "heap corruption - the game crashed";
                case 0xC00000FD:
                    return "a stack overflow - the game crashed";
                case 0xC000013A:
                    // Ctrl-C, or a console closing. Not the game's doing, and not the
                    // harness's either.
                    return "it was interrupted from the console";
                case 0x40010004:
                    return "it was ended by a debugger or a shutdown";
                case 0xFFFFFFFF:
                    // What TerminateProcess is asked for by Process.Kill, and so what the
                    // harness's own hard close leaves. The mod's engine reports the same
                    // code after the probe kills it on purpose, which is where the
                    // pairing was read off.
                    return "which is what killing a process leaves, rather than crashing";
                default:
                    return null;
            }
        }
    }
}
