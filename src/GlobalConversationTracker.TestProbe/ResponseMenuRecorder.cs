// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Text.Json;

namespace GlobalConversationTracker.TestProbe
{
    /// <summary>One option as the game finished composing it.</summary>
    internal sealed class RecordedOption
    {
        /// <summary>Creates a record.</summary>
        /// <param name="conversationId">The destination entry's conversation.</param>
        /// <param name="entryId">The destination entry.</param>
        /// <param name="text">The final text, markup and all.</param>
        /// <param name="check">Which kind of roll it is, or null for none.</param>
        internal RecordedOption(
            int? conversationId, int? entryId, string? text, string? check)
        {
            ConversationId = conversationId;
            EntryId = entryId;
            Text = text;
            Check = check;
        }

        /// <summary>The destination entry's conversation, or null if unreadable.</summary>
        internal int? ConversationId { get; }

        /// <summary>The destination entry, or null if unreadable.</summary>
        internal int? EntryId { get; }

        /// <summary>
        /// The final text, with every tag the game and the mod put on it.
        /// </summary>
        /// <remarks>
        /// Logged verbatim rather than stripped, because the colour is the answer: an
        /// option marked orange leads somewhere no save has reached, red somewhere this
        /// save has not, and an unmarked one leads nowhere better than itself. Strip the
        /// markup and all three read the same.
        /// </remarks>
        internal string? Text { get; }

        /// <summary>
        /// <c>white</c>, <c>red</c>, or null where the option rolls nothing.
        /// </summary>
        /// <remarks>
        /// FROM THE GAME'S OWN ENTRY, not from what the mod drew on it. The mod draws a
        /// Pass / Fail line under exactly the rolled checks, so a test that read "is this
        /// a check" out of the mod's own output could only ever confirm the mod agreed
        /// with itself. Read here, it is an independent fact to hold the mod to - and one
        /// that needs no entry ids pinned in a suite, which matters because which options
        /// a conversation offers is not stable between runs.
        /// </remarks>
        internal string? Check { get; }
    }

    /// <summary>
    /// Collects a response menu's options and reports them as one event.
    /// </summary>
    /// <remarks>
    /// <para>One event per menu rather than one per option, because what a test asks is
    /// about the menu: which of these options is marked, and which is not. Options
    /// arriving as separate lines would have to be stitched back together by the
    /// reader, and it would have no reliable way to know it had them all.</para>
    ///
    /// <para>The game composes each option's text through <c>ChooseResponseText</c> and
    /// announces the menu through <c>OnConversationResponseMenu</c>, and this does not
    /// assume an order between them: the menu supplies the count, the options fill a
    /// buffer, and whichever completes the pair emits. That matters because the order
    /// is not visible in the decompiled source - the bodies are stubbed out - and a
    /// recorder that guessed would emit either nothing or half a menu.</para>
    /// </remarks>
    internal sealed class ResponseMenuRecorder
    {
        private readonly List<RecordedOption> _options = new List<RecordedOption>();
        private bool _expecting;
        private int _expected;
        private int? _conversationId;
        private int? _money;

        /// <summary>Records one composed option.</summary>
        /// <param name="option">The option.</param>
        internal void AddOption(RecordedOption option)
        {
            _options.Add(option);
            EmitIfComplete();
        }

        /// <summary>Records that a menu of a given size is being shown.</summary>
        /// <param name="count">How many options it holds.</param>
        /// <param name="conversationId">The conversation it belongs to.</param>
        /// <param name="money">The balance the look-ahead crawled from.</param>
        internal void MenuShown(int count, int? conversationId, int? money)
        {
            // A previous menu that never completed would otherwise have its options
            // counted towards this one.
            Flush("superseded");

            _expecting = true;
            _expected = count;
            _conversationId = conversationId;
            _money = money;
            EmitIfComplete();
        }

        /// <summary>
        /// Reports whatever has been collected but not yet emitted, and forgets it.
        /// </summary>
        /// <remarks>
        /// Called when a conversation ends and when a new menu supersedes an unfinished
        /// one. A buffer that silently vanished would make a missing option look like an
        /// option the game never offered, which is the same shape as the bug these tests
        /// hunt.
        /// </remarks>
        /// <param name="why">What ended the menu, for the log.</param>
        internal void Flush(string why)
        {
            if (_options.Count == 0 && !_expecting)
            {
                return;
            }

            Emit(why);
        }

        private void EmitIfComplete()
        {
            if (_expecting && _options.Count >= _expected)
            {
                // Counted before it is written, so that anything asking "is a menu up"
                // gets the answer as soon as the menu is whole rather than a poll later.
                TestProbePlugin.NoteMenu();
                Emit("complete");
            }
        }

        private void Emit(string state)
        {
            RecordedOption[] options = _options.ToArray();
            int? conversationId = _conversationId;
            int? money = _money;
            bool expecting = _expecting;
            int expected = _expected;

            _options.Clear();
            _expecting = false;
            _expected = 0;
            _conversationId = null;
            _money = null;

            ProbeLog.Write("menu", writer =>
            {
                ProbeLog.WriteFields(
                    writer,
                    "conversation", conversationId,
                    "money", money,
                    "state", state,
                    "expected", expecting ? expected : (int?)null,
                    "shown", options.Length,
                    // WHAT THE CHECK RULE WOULD HAVE READ, at the moment the menu it is
                    // marking goes up. The save-applied event carries the same reading
                    // taken mid-load; the pair is what says whether a difference from the
                    // committed sheet is the load not having finished or the game valuing
                    // the character differently. See de-2p8j.5.
                    "skills", TestProbePlugin.Skills(),
                    // AND WHAT THE SCENE GUARDS READ, which has to be taken here rather
                    // than at save-applied to mean anything: the Lua tables are in by then
                    // but the area is not, so a mid-load reading pairs one save's weather
                    // with the previous save's scene. See de-kxam.1.
                    "scene", TestProbePlugin.Scene(),
                    // The same two readings the line event carries, so the pair can be
                    // compared: whichever of them says something different here from what
                    // it said while a line was up is the one that can replace the
                    // harness's fixed wait (de-6vyj).
                    "continueButton", DialogueWaitProbe.ContinueButton(),
                    "toggle", DialogueWaitProbe.Toggle());

                writer.WriteStartArray("options");
                foreach (RecordedOption option in options)
                {
                    writer.WriteStartObject();
                    ProbeLog.WriteFields(
                        writer,
                        "conversation", option.ConversationId,
                        "entry", option.EntryId,
                        "text", option.Text,
                        "check", option.Check);
                    writer.WriteEndObject();
                }

                writer.WriteEndArray();
            });
        }
    }
}
