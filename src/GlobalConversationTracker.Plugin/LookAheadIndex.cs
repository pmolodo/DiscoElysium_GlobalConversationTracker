// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;
using PixelCrushers.DialogueSystem;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The conversation index the look-ahead reads, kept honest against the game that is
    /// actually running.
    /// </summary>
    /// <remarks>
    /// <para>THE SHIPPED INDEX IS A CACHE, not ground truth. The plugin's answers are about
    /// the dialogue database the player's game loaded; the index is a pre-computed copy of
    /// it, and it is valid exactly while the two agree. A game patch, a localisation or
    /// another mod ends that, and an index that is trusted anyway answers confidently about
    /// a conversation the player is not in.</para>
    ///
    /// <para>CHECKED PER GROUP, ON FIRST USE. A whole-database check at load would have to
    /// walk 112,962 entries out of the PixelCrushers object graph before the main menu
    /// draws. The distribution says it is not necessary: the median conversation is 436
    /// bytes and two entries, and only the handful a crawl actually loads are large. A group
    /// is six conversations of a few hundred kilobytes, checked at a moment when the
    /// look-ahead is about to spend far more than that anyway - and checked once, because
    /// the answer cannot change while the game is running.</para>
    ///
    /// <para>THE GROUP IS THE UNIT, not the conversation. A crawl loads a whole group -
    /// six conversations for 631 - so all six have to hold before any answer over that group
    /// can be trusted.</para>
    ///
    /// <para>WHAT A MISMATCH COSTS is one rebuild, from the live database, written where the
    /// mod keeps its own files so the next launch finds it and pays nothing. The mod then
    /// works on a game it has never seen, which is the whole reason the bundled copy is only
    /// a startup optimisation.</para>
    /// </remarks>
    internal sealed class LookAheadIndex : IDisposable
    {
        /// <summary>The prefix every line here starts with, so a harness can find them.</summary>
        internal const string LogPrefix = "Look-ahead index:";

        /// <summary>What a rebuilt index is called, in the mod's own folder.</summary>
        /// <remarks>
        /// Beside the global state rather than beside the plugin, because the plugin's
        /// folder is a game install: a deploy clears out what it wrote last time, and a
        /// player without write access to it would have nowhere to put this.
        /// </remarks>
        internal const string RebuiltFileName = "GlobalConversationTracker.Index.rebuilt.jsonl";

        private readonly IGlobalStateLog _log;
        private readonly string _rebuiltPath;
        private readonly string? _variablesPath;

        /// <summary>
        /// What has already been checked, by group, and what it said.
        /// </summary>
        /// <remarks>
        /// Keyed by the whole group rather than by conversation, because that is the unit
        /// the answer is about; a conversation appearing in two groups is checked once per
        /// group and that is cheap enough not to be worth outsmarting.
        /// </remarks>
        private readonly Dictionary<string, bool> _checked =
            new Dictionary<string, bool>(StringComparer.Ordinal);

        private LookAheadLibrary _engine;

        private LookAheadIndex(
            LookAheadLibrary engine, string rebuiltPath, string? variablesPath, IGlobalStateLog log)
        {
            _engine = engine;
            _rebuiltPath = rebuiltPath;
            _variablesPath = variablesPath;
            _log = log;
        }

        /// <summary>The library, over whichever index is currently trusted.</summary>
        /// <remarks>
        /// Re-read after every <see cref="IsValidFor"/>: a rebuild replaces it, and a
        /// caller holding the old one would be asking the file that was just found wrong.
        /// </remarks>
        internal LookAheadLibrary Engine => _engine;

        /// <summary>
        /// Which index this is, counting from zero and incremented by every rebuild.
        /// </summary>
        /// <remarks>
        /// So a caller that CACHES anything derived from the index can tell when to throw
        /// it away. The patch keeps a questions list per conversation, which is safe because
        /// the questions cannot change while the game runs - but a rebuild changes the file
        /// they came from, and a cached list from the old one would be answered positionally
        /// against the new one's.
        /// </remarks>
        internal int Generation { get; private set; }

        /// <summary>
        /// Opens the best index available, or null if none of them opens.
        /// </summary>
        /// <param name="pluginDirectory">Where the shipped index was deployed.</param>
        /// <param name="modDirectory">Where the mod keeps its own files.</param>
        /// <param name="log">Where the result is reported.</param>
        /// <remarks>
        /// A rebuilt index is preferred over the shipped one. It was written from a database
        /// this machine actually loaded, so where both exist the shipped copy is the one
        /// already known to have been wrong.
        /// </remarks>
        internal static LookAheadIndex? Open(
            string pluginDirectory, string modDirectory, IGlobalStateLog log)
        {
            string rebuiltPath = Path.Combine(modDirectory, RebuiltFileName);
            string shippedPath = Path.Combine(pluginDirectory, NativeEngineCheck.IndexFileName);
            string? variablesPath = Path.Combine(
                pluginDirectory, NativeEngineCheck.VariablesFileName);
            if (!File.Exists(variablesPath))
            {
                variablesPath = null;
            }

            foreach (string candidate in new[] { rebuiltPath, shippedPath })
            {
                if (!File.Exists(candidate))
                {
                    continue;
                }

                try
                {
                    LookAheadLibrary engine = LookAheadLibrary.Open(candidate, variablesPath);
                    log.Info(
                        $"{LogPrefix} opened {Path.GetFileName(candidate)}, "
                        + $"{engine.ConversationCount} conversations, format {engine.IndexFormat}.");
                    return new LookAheadIndex(engine, rebuiltPath, variablesPath, log);
                }
                catch (Exception error)
                {
                    log.Warning(
                        $"{LogPrefix} {candidate} would not open "
                        + $"({error.GetType().Name}: {error.Message}).");
                }
            }

            log.Warning($"{LogPrefix} no index could be opened; the look-ahead has no graph.");
            return null;
        }

        /// <summary>
        /// Whether the open index still describes this group, rebuilding it if it does not.
        /// </summary>
        /// <param name="group">
        /// Every conversation the group covers, as the engine reported it.
        /// </param>
        /// <returns>
        /// False only where the index disagreed AND could not be rebuilt - which is the one
        /// case in which an answer over this group should not be trusted at all.
        /// </returns>
        internal bool IsValidFor(IReadOnlyList<int> group)
        {
            if (group == null || group.Count == 0)
            {
                return false;
            }

            string key = Key(group);
            if (_checked.TryGetValue(key, out bool remembered))
            {
                return remembered;
            }

            bool valid = Check(group, key);
            _checked[key] = valid;
            return valid;
        }

        /// <summary>Checks a group for the first time, and acts on what it finds.</summary>
        private bool Check(IReadOnlyList<int> group, string key)
        {
            long began = Stopwatch.GetTimestamp();
            int? disagreed = FirstDisagreement(group, out bool comparable);
            double elapsed = Milliseconds(began);

            if (!comparable)
            {
                // No hashes to compare against: the deployed file is the full index, which
                // is a build intermediate rather than a cache. Nothing can be said about
                // it, and it is trusted - which is exactly what the mod did before there
                // was any such thing as validation.
                _log.Info(
                    $"{LogPrefix} {key} cannot be checked - the index carries no hashes. "
                    + "Using it as it is.");
                return true;
            }

            if (disagreed == null)
            {
                _log.Info(
                    $"{LogPrefix} {key} matches the loaded database ({elapsed:N0} ms).");
                return true;
            }

            _log.Warning(
                $"{LogPrefix} conversation {disagreed} is not what the index says it is "
                + $"({elapsed:N0} ms). {Explain(disagreed.Value)} Rebuilding.");

            return Rebuild();
        }

        /// <summary>
        /// As much as can be said cheaply about WHY a conversation disagreed.
        /// </summary>
        /// <remarks>
        /// A hash that differs says only that something differs, and the two sides cannot
        /// be diffed here - the index carries the extractor's hash and not the string it
        /// was taken over. What CAN be compared is the shape, and the shape is where the
        /// difference usually is: a conversation the two disagree about the size of is a
        /// different conversation, and nothing about field formatting need be suspected.
        /// </remarks>
        private string Explain(int conversation)
        {
            int stored = _engine.EntryCount(conversation);
            int live = LiveDialogueDatabase.EntryCountOf(conversation);

            if (stored < 0)
            {
                return "The index does not hold that conversation at all.";
            }

            if (live < 0)
            {
                return "The loaded database does not hold that conversation at all.";
            }

            return stored == live
                ? $"Both hold {stored} entries, so the difference is in their content - a "
                    + "guard, a script, a link or a field."
                : $"The index holds {stored} entries and the loaded database holds {live}.";
        }

        /// <summary>
        /// The first conversation whose live content is not what the index stored, or null
        /// if they all match.
        /// </summary>
        /// <param name="group">The conversations to check.</param>
        /// <param name="comparable">
        /// False where the index carries no hash at all, in which case nothing was
        /// compared.
        /// </param>
        private int? FirstDisagreement(IReadOnlyList<int> group, out bool comparable)
        {
            comparable = false;
            foreach (int conversation in group)
            {
                string stored;
                try
                {
                    stored = _engine.HashOf(conversation);
                }
                catch (InvalidOperationException)
                {
                    // The index does not hold a conversation the group named, which can
                    // only mean the group came from somewhere else. That is a mismatch.
                    return conversation;
                }

                if (stored.Length == 0)
                {
                    continue;
                }

                comparable = true;
                if (LiveDialogueDatabase.HashOf(conversation) != stored)
                {
                    return conversation;
                }
            }

            return null;
        }

        /// <summary>
        /// Writes a new index from the live database and reopens over it.
        /// </summary>
        /// <remarks>
        /// The WHOLE database, not the group that disagreed. A patch does not change one
        /// conversation, the next group would pay for the same discovery again, and the
        /// point of writing it where the mod keeps its files is that the rebuild is paid
        /// once and never again.
        /// </remarks>
        private bool Rebuild()
        {
            long began = Stopwatch.GetTimestamp();
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(_rebuiltPath)!);
                int written = ShippedIndexWriter.Write(_rebuiltPath, LiveConversations());
                double writing = Milliseconds(began);

                LookAheadLibrary rebuilt = LookAheadLibrary.Open(_rebuiltPath, _variablesPath);
                _engine.Dispose();
                _engine = rebuilt;
                Generation++;

                // Everything decided against the old file is now about a file that no
                // longer exists, including groups that had matched it.
                _checked.Clear();

                _log.Info(
                    $"{LogPrefix} rebuilt {written} conversations from the loaded database "
                    + $"in {writing:N0} ms, at {_rebuiltPath}. It will be used from now on, "
                    + "and found on the next launch.");
                return true;
            }
            catch (Exception error)
            {
                // A rebuild that fails leaves the old index open and in use. That is the
                // honest fallback: its answers are about a database that has moved, which
                // costs a wrong marker, where refusing to answer at all costs the feature.
                _log.Warning(
                    $"{LogPrefix} could not rebuild the index "
                    + $"({error.GetType().Name}: {error.Message}). "
                    + "The look-ahead will keep using the one it has.");
                return false;
            }
        }

        /// <summary>Every conversation in the loaded database, as an index carries it.</summary>
        private static IEnumerable<IndexConversation> LiveConversations()
        {
            DialogueDatabase database = DialogueManager.masterDatabase
                ?? throw new InvalidOperationException(
                    "There is no dialogue database loaded to rebuild an index from.");

            Il2CppSystem.Collections.Generic.List<Conversation> conversations =
                database.conversations
                ?? throw new InvalidOperationException(
                    "The loaded dialogue database holds no conversations.");

            for (int index = 0; index < conversations.Count; index++)
            {
                Conversation conversation = conversations[index];
                if (conversation != null)
                {
                    yield return LiveDialogueDatabase.Read(conversation);
                }
            }
        }

        /// <summary>How a group is named in the log and in what has been checked.</summary>
        private static string Key(IReadOnlyList<int> group)
        {
            return "group " + string.Join(",", group);
        }

        /// <summary>Wall time since a stopwatch timestamp, in milliseconds.</summary>
        private static double Milliseconds(long since)
        {
            return (Stopwatch.GetTimestamp() - since) * 1000d / Stopwatch.Frequency;
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            _engine.Dispose();
        }
    }
}
