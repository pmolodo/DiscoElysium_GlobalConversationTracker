// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Core;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// The dialogue statuses a loaded save records, read through the library the mod links.
    /// </summary>
    /// <remarks>
    /// <para>ONE DEFINITION OF THE SAVE FORMAT. This used to be a Lua blob parser here, in
    /// C#, beside the one in Rust that every other reader in the repository goes through -
    /// and a format stated twice drifts, in whichever copy no test happens to exercise. The
    /// library the plugin already links for the state file reads this too, because both are
    /// things the RUNNING GAME reads and neither can answer for a process that will not
    /// start.</para>
    ///
    /// <para>WHAT THAT COSTS, measured rather than assumed: the Rust reader builds the whole
    /// table where this one walked past it, which is 62 ms and 30 MB held over a real 8.1 MB
    /// save. It lands on a save load, where the player is already waiting seconds. The
    /// alternative was a second streaming reader of the blob in Rust, which would have been
    /// the same duplication in a different language.</para>
    ///
    /// <para>ONLY WHAT IS ABOVE UNTOUCHED COMES BACK, which is around 1,500 rows of a real
    /// save's 113,000. The rest were walked and stored nowhere; not carrying them across is
    /// the same outcome and none of the array.</para>
    /// </remarks>
    public static class SaveStatuses
    {
        /// <summary>
        /// Reads every SimStatus above Untouched out of a save's <c>ntwtf.lua</c> bytes.
        /// </summary>
        /// <param name="blob">The blob, as stored inside the save.</param>
        /// <param name="failure">Why nothing came back, where nothing did.</param>
        /// <returns>The rows, or an empty list when the blob could not be read.</returns>
        public static List<SimStatusRow> Read(ReadOnlySpan<byte> blob, out string? failure)
        {
            IntPtr read;
            unsafe
            {
                fixed (byte* bytes = blob)
                {
                    read = GlobalStateNative.ReadSave(bytes, (nuint)blob.Length);
                }
            }

            if (read == IntPtr.Zero)
            {
                failure = "The save reader could not be reached.";
                return new List<SimStatusRow>();
            }

            try
            {
                if (GlobalStateNative.Outcome(read) != GlobalStateNative.OutcomeLoaded)
                {
                    failure = GlobalStateNative.TextAt(
                        GlobalStateNative.Message(read, out nuint length), length);
                    return new List<SimStatusRow>();
                }

                failure = null;
                return RowsIn(read);
            }
            finally
            {
                GlobalStateNative.FreeRead(read);
            }
        }

        /// <summary>Everything a read holds, copied out in one call.</summary>
        private static List<SimStatusRow> RowsIn(IntPtr read)
        {
            int count = checked((int)GlobalStateNative.EntryCount(read));
            var entries = new GlobalStateNative.Entry[count];
            if (count > 0)
            {
                unsafe
                {
                    fixed (GlobalStateNative.Entry* into = entries)
                    {
                        GlobalStateNative.Entries(read, into, (nuint)count);
                    }
                }
            }

            var rows = new List<SimStatusRow>(count);
            foreach (GlobalStateNative.Entry entry in entries)
            {
                rows.Add(new SimStatusRow(
                    entry.Conversation,
                    entry.DialogueEntry,
                    SimStatusNames.ToGameString((SimStatus)entry.Status)));
            }

            return rows;
        }
    }
}
