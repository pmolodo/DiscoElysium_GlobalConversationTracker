// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using GlobalConversationTracker.Persistence;

namespace GlobalConversationTracker.CrashHarness
{
    /// <summary>
    /// Saves a global state and, on request, kills its own process part way
    /// through, at a named step of <see cref="GlobalStateStore.Save"/>.
    /// </summary>
    /// <remarks>
    /// This is how the crash points get tested rather than asserted. An in-process
    /// test that throws from a hook still unwinds the stack, runs finally blocks and
    /// flushes buffers on the way out; <c>Process.Kill</c> on a real child process
    /// does none of that, so whatever is on disk afterwards is what a genuine crash
    /// leaves behind.
    /// </remarks>
    public static class Program
    {
        /// <summary>Exit code when a requested crash step was never reached.</summary>
        public const int ExitStepNotReached = 2;

        /// <summary>Exit code for a usage error.</summary>
        public const int ExitBadUsage = 3;

        /// <summary>The argument value meaning "do not crash".</summary>
        public const string NoCrashStep = "none";

        /// <summary>
        /// Chunk size used for the temp-file write, small enough that the payload
        /// takes many chunks and the mid-write crash point is reachable.
        /// </summary>
        internal const int WriteChunkSize = 16;

        /// <summary>Entry point. Usage: <c>&lt;directory&gt; &lt;payload&gt; &lt;step|none&gt;</c>.</summary>
        public static int Main(string[] args)
        {
            if (args.Length != 3)
            {
                Console.Error.WriteLine("Usage: <save-game-directory> <payload> <crash-step|none>");
                return ExitBadUsage;
            }

            string directory = args[0];
            string payloadName = args[1];
            string stepName = args[2];

            GlobalConversationState state = CrashHarnessPayloads.Build(payloadName);
            var store = new GlobalStateStore(directory);
            store.WriteChunkSize = WriteChunkSize;

            bool crashRequested = !string.Equals(stepName, NoCrashStep, StringComparison.Ordinal);
            GlobalStateSaveStep crashStep = default;
            if (crashRequested)
            {
                if (!Enum.TryParse(stepName, ignoreCase: false, out crashStep))
                {
                    Console.Error.WriteLine($"Unknown crash step '{stepName}'.");
                    return ExitBadUsage;
                }

                store.SaveStepHook = step =>
                {
                    if (step != crashStep)
                    {
                        return;
                    }

                    // Hard kill: no unwinding, no finally blocks, no managed flush.
                    Process.GetCurrentProcess().Kill();

                    // Kill is asynchronous with respect to this thread on some platforms;
                    // block so the save cannot advance past the requested step.
                    System.Threading.Thread.Sleep(TimeSpan.FromMinutes(1));
                };
            }

            store.Save(state);

            if (crashRequested)
            {
                Console.Error.WriteLine($"Crash step '{stepName}' was never reached; the save completed.");
                return ExitStepNotReached;
            }

            return 0;
        }
    }
}
