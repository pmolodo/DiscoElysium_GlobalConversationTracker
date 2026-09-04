// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text.Json;

namespace GlobalConversationTracker.Engine
{
    /// <summary>What one option scored.</summary>
    /// <param name="Start">The option entry that was asked about.</param>
    /// <param name="Best">
    /// The best novelty reachable from it: 0 seen, 1 unseen this save, 2 unseen in any
    /// save. The same numbering as the managed engine's <c>Novelty</c>, deliberately, so
    /// the two can be compared while both exist.
    /// </param>
    /// <param name="Complete">
    /// Whether the search settled. False means <paramref name="Best"/> is a LOWER BOUND -
    /// the crawl ran out of budget, and something better may be reachable. A marker drawn
    /// from an incomplete answer is not wrong, only possibly too modest.
    /// </param>
    /// <param name="ElapsedMs">How long the crawl for this option took.</param>
    /// <param name="StatesExplored">
    /// How many search states it explored. What the diagnostics are really about: a time
    /// alone cannot say whether a menu was slow because the search was large or because the
    /// machine was busy, and this is the half that is the same on both.
    /// </param>
    /// <param name="NodesReached">How many entries it reached.</param>
    /// <param name="StoppedBy">
    /// What stopped it: "none", "states" or "time". More than
    /// <paramref name="Complete"/> says, and the difference is the one a player tuning the
    /// budgets needs - a crawl out of STATES wants a bigger state budget, one out of TIME
    /// on the same states wants a longer clock.
    /// </param>
    /// <param name="Branches">
    /// The option's two outcomes, where it is a white or red check, and null on everything
    /// else. The absence is what the mod reads to decide whether an option earns a
    /// Pass/Fail line at all - see de-fes. The fields above are the two combined, so a
    /// caller that ignores this still gets the right answer for the option as a whole.
    /// </param>
    public readonly record struct LookAheadAnswer(
        NodeRef Start,
        int Best,
        bool Complete,
        long ElapsedMs,
        long StatesExplored,
        long NodesReached,
        string StoppedBy,
        BranchAnswers? Branches = null);

    /// <summary>What the search found down one outcome of a rolled check.</summary>
    /// <param name="Destination">
    /// The best novelty among the entries this outcome leads to DIRECTLY - what the mod
    /// colours the word "Pass" or "Fail" by.
    /// </param>
    /// <param name="Best">
    /// The best novelty anywhere down this outcome, which earns it an asterisk when it
    /// beats <paramref name="Destination"/> - the same rule an option's own marker follows.
    /// </param>
    /// <param name="Complete">
    /// Whether this outcome's search settled. An outcome that gave up draws the uncertain
    /// marker rather than nothing, for the reason de-pvq gives.
    /// </param>
    public readonly record struct BranchAnswer(int Destination, int Best, bool Complete);

    /// <summary>Both outcomes of a rolled check.</summary>
    public readonly record struct BranchAnswers(BranchAnswer Pass, BranchAnswer Fail);

    /// <summary>What the library said about a whole menu.</summary>
    /// <remarks>
    /// A request the engine could not serve at all comes back with <see cref="Error"/> set
    /// and no answers, rather than as a thrown exception, so a caller has ONE thing to
    /// parse and one place to look. What throws is what happens before there is a response
    /// at all: a request that is not JSON, or a library that is not there.
    /// </remarks>
    public sealed class LookAheadResponse
    {
        private LookAheadResponse(IReadOnlyList<LookAheadAnswer> answers, string? error)
        {
            Answers = answers;
            Error = error;
        }

        /// <summary>One per start, in the order they were asked about.</summary>
        public IReadOnlyList<LookAheadAnswer> Answers { get; }

        /// <summary>Why the whole request failed, or null if it did not.</summary>
        public string? Error { get; }

        /// <summary>Reads what <c>gct_look_ahead</c> returned.</summary>
        /// <param name="json">The library's answer.</param>
        /// <exception cref="ArgumentNullException">The JSON is null.</exception>
        /// <exception cref="FormatException">It is not a response document.</exception>
        public static LookAheadResponse Parse(string json)
        {
            if (json == null)
            {
                throw new ArgumentNullException(nameof(json));
            }

            try
            {
                using JsonDocument document = JsonDocument.Parse(json);
                JsonElement root = document.RootElement;

                string? error = root.TryGetProperty("error", out JsonElement reason)
                    && reason.ValueKind == JsonValueKind.String
                        ? reason.GetString()
                        : null;

                var answers = new List<LookAheadAnswer>();
                if (root.TryGetProperty("answers", out JsonElement listed)
                    && listed.ValueKind == JsonValueKind.Array)
                {
                    foreach (JsonElement answer in listed.EnumerateArray())
                    {
                        JsonElement start = answer.GetProperty("start");
                        answers.Add(new LookAheadAnswer(
                            new NodeRef(
                                start.GetProperty("conversation").GetInt32(),
                                start.GetProperty("entry").GetInt32()),
                            answer.GetProperty("best").GetInt32(),
                            answer.GetProperty("complete").GetBoolean(),
                            answer.GetProperty("elapsed_ms").GetInt64(),
                            Number(answer, "states_explored"),
                            Number(answer, "nodes_reached"),
                            Text(answer, "stopped_by"),
                            Branches(answer)));
                    }
                }

                return new LookAheadResponse(answers, error);
            }
            catch (JsonException error)
            {
                throw new FormatException(
                    "the look-ahead library's answer could not be read: " + error.Message,
                    error);
            }
        }

        /// <summary>One optional number, or zero where the library did not send it.</summary>
        /// <remarks>
        /// Absent reads as zero rather than as a parse failure. These are diagnostics, and a
        /// library built before they existed should still answer questions rather than
        /// refuse them.
        /// </remarks>
        /// <summary>
        /// A rolled check's two outcomes, or null where the option has only one.
        /// </summary>
        /// <remarks>
        /// A MISSING PAIR IS NOT A MALFORMED ANSWER. The engine writes this only for a
        /// white or red check, so its absence carries meaning - the option is not a roll -
        /// and an older library that has never heard of branches reads as exactly that.
        /// </remarks>
        private static BranchAnswers? Branches(JsonElement answer)
        {
            if (!answer.TryGetProperty("branches", out JsonElement pair)
                || pair.ValueKind != JsonValueKind.Object)
            {
                return null;
            }

            return new BranchAnswers(Branch(pair, "pass"), Branch(pair, "fail"));
        }

        /// <summary>One outcome, read defensively: a missing half reads as reaching nothing.</summary>
        private static BranchAnswer Branch(JsonElement pair, string name)
        {
            if (!pair.TryGetProperty(name, out JsonElement branch)
                || branch.ValueKind != JsonValueKind.Object)
            {
                return new BranchAnswer(0, 0, true);
            }

            return new BranchAnswer(
                (int)Number(branch, "destination"),
                (int)Number(branch, "best"),
                !branch.TryGetProperty("complete", out JsonElement complete)
                    || complete.ValueKind != JsonValueKind.False);
        }

        private static long Number(JsonElement answer, string name)
        {
            return answer.TryGetProperty(name, out JsonElement value)
                && value.ValueKind == JsonValueKind.Number
                ? value.GetInt64()
                : 0;
        }

        /// <summary>One optional string, or empty where the library did not send it.</summary>
        private static string Text(JsonElement answer, string name)
        {
            return answer.TryGetProperty(name, out JsonElement value)
                && value.ValueKind == JsonValueKind.String
                ? value.GetString() ?? string.Empty
                : string.Empty;
        }
    }
}
