// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using GlobalConversationTracker.LookAhead;

namespace GlobalConversationTracker.LookAheadOffline
{
    internal static class Program
    {
        private static int Main(string[] args)
        {
            try
            {
                var options = Arguments.Parse(args);
                ConversationIndex index = ConversationIndex.Read(options.IndexPath);
                OfflineState state = JsonSerializer.Deserialize<OfflineState>(File.ReadAllText(options.StatePath),
                    new JsonSerializerOptions { PropertyNameCaseInsensitive = true })
                    ?? throw new InvalidDataException("The state file contains null.");
                var world = new OfflineWorld(state);
                LookAheadGraph graph = index.BuildGraph(options.ConversationId);
                IReadOnlyList<DialogueNodeId> starts = options.EntryId.HasValue
                    ? new[] { new DialogueNodeId(options.ConversationId, options.EntryId.Value) }
                    : index.EntriesIn(options.ConversationId);
                var results = new List<CrawlResult>(starts.Count);
                List<string>? samples = options.SampleNode == null ? null : new List<string>();
                var engine = new LookAheadEngine(new LookAheadOptions
                {
                    StateBudget = options.StateBudget,
                    TimeBudget = TimeSpan.Zero,
                    CollectTrace = true,
                    StateSampleInterval = options.SampleNode == null ? 0 : 1,
                    OnStateReached = (node, crawlState, _) => Capture(
                        node, crawlState, graph.Symbols, options.SampleNode, samples),
                    CounterCapForSlot = slot => state.CounterCaps.TryGetValue(
                        graph.Symbols.NameOf(slot), out int cap) ? cap : 16,
                });
                foreach (DialogueNodeId start in starts)
                {
                    if (!graph.Get(start).IsGroup)
                    {
                        LookAheadResult result = engine.Evaluate(graph, start, world, world.GetNovelty);
                        results.Add(new CrawlResult(start, result));
                    }
                }

                Console.WriteLine(JsonSerializer.Serialize(results, new JsonSerializerOptions
                {
                    WriteIndented = true,
                }));
                if (samples != null)
                {
                    File.WriteAllLines(options.SamplePath!, samples);
                }
                return 0;
            }
            catch (Exception exception)
            {
                Console.Error.WriteLine(exception);
                return 1;
            }
        }

        private static void Capture(
            DialogueNodeId node,
            LookAheadState state,
            StateSymbols symbols,
            DialogueNodeId? wanted,
            List<string>? samples)
        {
            if (samples == null || wanted == null || !node.Equals(wanted.Value) || samples.Count >= 100)
            {
                return;
            }

            var values = new List<string>();
            for (int slot = 0; slot < symbols.Count; slot++)
            {
                int value = state.Get(slot);
                if (value != 0)
                {
                    values.Add(symbols.NameOf(slot) + "=" + value);
                }
            }

            samples.Add($"{node.ConversationId}:{node.EntryId} money={state.Money} "
                + $"dayMinutes={state.DayMinutes} {string.Join(", ", values.OrderBy(value => value))}");
        }
    }

    internal sealed class CrawlResult
    {
        public CrawlResult(DialogueNodeId start, LookAheadResult result)
        {
            Conversation = start.ConversationId;
            Entry = start.EntryId;
            Best = result.Best.ToString();
            StatesExplored = result.StatesExplored;
            NodesReached = result.NodesReached;
            StoppedBy = result.StoppedBy.ToString();
        }

        public int Conversation { get; }
        public int Entry { get; }
        public string Best { get; }
        public int StatesExplored { get; }
        public int NodesReached { get; }
        public string StoppedBy { get; }
    }

    internal sealed class Arguments
    {
        private Arguments(
            string indexPath,
            string statePath,
            int conversationId,
            int? entryId,
            int stateBudget,
            DialogueNodeId? sampleNode,
            string? samplePath)
        {
            IndexPath = indexPath;
            StatePath = statePath;
            ConversationId = conversationId;
            EntryId = entryId;
            StateBudget = stateBudget;
            SampleNode = sampleNode;
            SamplePath = samplePath;
        }

        public string IndexPath { get; }
        public string StatePath { get; }
        public int ConversationId { get; }
        public int? EntryId { get; }
        public int StateBudget { get; }
        public DialogueNodeId? SampleNode { get; }
        public string? SamplePath { get; }

        public static Arguments Parse(string[] args)
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            for (int i = 0; i < args.Length; i += 2)
            {
                if (i + 1 >= args.Length || !args[i].StartsWith("--", StringComparison.Ordinal))
                {
                    throw new ArgumentException("Usage: --index PATH --state PATH --conversation ID [--entry ID] [--state-budget N]");
                }

                values.Add(args[i], args[i + 1]);
            }

            bool samplesRequested = values.ContainsKey("--sample-node") || values.ContainsKey("--sample-out");
            if (samplesRequested && (!values.ContainsKey("--sample-node") || !values.ContainsKey("--sample-out")))
            {
                throw new ArgumentException("--sample-node and --sample-out must be used together.");
            }

            return new Arguments(
                Required(values, "--index"),
                Required(values, "--state"),
                Integer(values, "--conversation"),
                OptionalInteger(values, "--entry"),
                values.ContainsKey("--state-budget") ? Integer(values, "--state-budget") : 200_000,
                samplesRequested ? Node(values["--sample-node"]) : null,
                samplesRequested ? values["--sample-out"] : null);
        }

        private static string Required(IReadOnlyDictionary<string, string> values, string name)
        {
            return values.TryGetValue(name, out string? value)
                ? value : throw new ArgumentException($"Missing required {name}.");
        }

        private static int Integer(IReadOnlyDictionary<string, string> values, string name)
        {
            return int.TryParse(Required(values, name), out int value)
                ? value : throw new ArgumentException($"{name} must be an integer.");
        }

        private static int? OptionalInteger(IReadOnlyDictionary<string, string> values, string name)
        {
            return values.ContainsKey(name) ? Integer(values, name) : null;
        }

        private static DialogueNodeId Node(string value)
        {
            string[] pieces = value.Split(':');
            if (pieces.Length != 2 || !int.TryParse(pieces[0], out int conversation)
                || !int.TryParse(pieces[1], out int entry))
            {
                throw new ArgumentException("--sample-node must be CONVERSATION:ENTRY.");
            }

            return new DialogueNodeId(conversation, entry);
        }
    }
}
