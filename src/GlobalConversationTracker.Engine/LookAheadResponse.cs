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
    public readonly record struct LookAheadAnswer(
        NodeRef Start, int Best, bool Complete, long ElapsedMs);

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
                            answer.GetProperty("elapsed_ms").GetInt64()));
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
    }
}
