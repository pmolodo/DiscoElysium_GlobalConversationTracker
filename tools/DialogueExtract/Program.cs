// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using GlobalConversationTracker.DialogueAsset;

namespace GlobalConversationTracker.DialogueExtract
{
    /// <summary>Reads what the offline tools need out of the Dialogue System database.</summary>
    internal static class Program
    {
        private const string ArticyIdsCommand = "articy-ids";
        private const string ConversationIndexCommand = "conversation-index";
        private const string CorpusCommand = "corpus";
        private const string VariablesCommand = "variables";
        private const string ActorsCommand = "actors";
        private const string ItemNamesCommand = "item-names";
        private const string ShippedIndexCommand = "shipped-index";
        private const string WorstCaseStateCommand = "worst-case-state";
        private const int ExitFailure = 1;

        private const string Usage = """
            DialogueExtract - read a Disco Elysium Dialogue System database .asset.

            Usage:
              dotnet run --project tools/DialogueExtract -- <command> [options]

            Commands:
              articy-ids          The articy id of every conversation and dialogue entry,
                                  as the two maps the engine host reads to rebuild a
                                  Conversation_SimX_* strings.
              conversation-index  One compact JSON object per conversation, one per line:
                                  its id, title, actor, conversant, and every dialogue
                                  entry with its guard, script, links and fields.
              corpus              Every distinct guard and script the database contains,
                                  sorted, one per line, as distinct_guards.txt and
                                  distinct_scripts.txt.
              variables           The database's variable table - name, declared type and
                                  initial value, one JSON object per line - so a world can
                                  tell a counter from a flag.
              actors              The database's actor table - id and name, one JSON object
                                  per line - so an entry's Actor field can be read as the
                                  skill a passive check tests.
              item-names          The database's item table with each item's English display
                                  name from the lockit - id, stack name and display name, one
                                  JSON object per line - so a reader with only a save can
                                  answer CheckItem: the key pocket a save writes is display
                                  names, and what a key is called is in neither the save nor
                                  the database.
              shipped-index       The index as the mod ships it: the same records with
                                  everything no crawl reads removed. Reads the index that
                                  conversation-index wrote.
              worst-case-state    The global state that makes a look-ahead crawl as
                                  expensive as it can be: every entry of every
                                  conversation in the index recorded as WasDisplayed.

            Options:
              --asset PATH    Every command but shipped-index and worst-case-state: the
                              database .asset. Default:
                              .game_reference_copies/AssetRipperExport/ExportedProject/Assets/Dialogue Databases/Disco Elysium.asset
              --index PATH    worst-case-state: the index conversation-index wrote. Default:
                              .game_reference_copies/derived/conversation_index.jsonl
              --lockit PATH   item-names: the English lockit the display names are in.
                              Default:
                              .game_reference_copies/AssetRipperExport/ExportedProject/Assets/Resources/lockits/english/GeneralLockitEnglish.asset
              --out PATH      Where to write the output. Defaults:
                              articy-ids          articy_ids_final_cut.json
                              conversation-index  .game_reference_copies/derived/conversation_index.jsonl
                              worst-case-state    testing/scenarios/global-state-worst-case.json
              --out-dir PATH  corpus, variables, actors, item-names: the directory to write
                              into. Default: .game_reference_copies/derived
              -h, --help      Show this message.
            """;

        private static readonly string DefaultAsset = Path.Combine(".game_reference_copies", "AssetRipperExport",
            "ExportedProject", "Assets", "Dialogue Databases", "Disco Elysium.asset");

        private static readonly string DefaultDerived = Path.Combine(".game_reference_copies", "derived");

        private static readonly string DefaultLockit = Path.Combine(".game_reference_copies",
            "AssetRipperExport", "ExportedProject", "Assets", "Resources", "lockits", "english",
            "GeneralLockitEnglish.asset");

        private static readonly string DefaultOut = Path.Combine(DefaultDerived,
            "conversation_index.jsonl");

        private static readonly string DefaultIndex = DefaultOut;

        private static readonly string DefaultStateOut = Path.Combine("testing", "scenarios",
            "global-state-worst-case.json");

        // At the repository root, where the engine host looks for it by name.
        private static readonly string DefaultArticyIdsOut = "articy_ids_final_cut.json";

        private static int Main(string[] args)
        {
            try
            {
                return Run(args);
            }
            // InvalidDataException is listed on its own: it says the input file is not
            // what it claims to be, which is the caller's problem too, but it descends
            // from SystemException rather than IOException.
            catch (Exception exception) when (exception is ArgumentException or IOException
                or InvalidDataException)
            {
                // Bad input is the caller's problem, not a defect: say what is wrong and
                // stop. A stack trace here would bury the one line that helps.
                Console.Error.WriteLine($"error: {exception.Message}");
                return ExitFailure;
            }
            catch (Exception exception)
            {
                Console.Error.WriteLine(exception);
                return ExitFailure;
            }
        }

        private static int Run(string[] args)
        {
            if (args.Length == 0 || args[0] is "-h" or "--help")
            {
                Console.WriteLine(Usage);
                return 0;
            }

            string command = args[0];
            switch (command)
            {
                case ArticyIdsCommand:
                    return ArticyIds(ParseOptions(args, command));
                case ConversationIndexCommand:
                    return ConversationIndex(ParseOptions(args, command));
                case CorpusCommand:
                    return Corpus(ParseOptions(args, command));
                case VariablesCommand:
                    return Variables(ParseOptions(args, command));
                case ActorsCommand:
                    return Actors(ParseOptions(args, command));
                case ItemNamesCommand:
                    return ItemNames(ParseOptions(args, command));
                case ShippedIndexCommand:
                    return TrimmedIndex(ParseOptions(args, command));
                case WorstCaseStateCommand:
                    return WorstCaseState(ParseOptions(args, command));
                default:
                    throw new ArgumentException($"Unknown command '{command}'\n\n{Usage}");
            }
        }

        private static int ArticyIds(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string outPath = Option(options, "--out", DefaultArticyIdsOut);
            RejectUnknownOptions(options);
            PrepareOutput(outPath);

            ArticyIdIndex index = ArticyIdIndex.Build(asset);
            ArticyIdFile.Write(outPath, index);
            Console.WriteLine($"wrote {index.Conversations.Count} conversation and "
                + $"{index.DialogueEntries.Count} dialogue entry articy ids to {outPath}");
            return 0;
        }

        private static int ConversationIndex(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string outPath = Option(options, "--out", DefaultOut);
            RejectUnknownOptions(options);
            PrepareOutput(outPath);

            int written = ConversationIndexFile.Write(outPath, ConversationIndexExtractor.Extract(asset));
            Console.WriteLine($"wrote {written} conversations to {outPath}");
            return 0;
        }

        private static int Corpus(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string outDir = Option(options, "--out-dir", DefaultDerived);
            RejectUnknownOptions(options);
            Directory.CreateDirectory(outDir);

            DialogueCorpus corpus = DialogueCorpusExtractor.Extract(asset);

            string guardPath = Path.Combine(outDir, DialogueCorpusFile.GuardFileName);
            string scriptPath = Path.Combine(outDir, DialogueCorpusFile.ScriptFileName);
            DialogueCorpusFile.Write(guardPath, corpus.Guards);
            DialogueCorpusFile.Write(scriptPath, corpus.Scripts);

            Console.WriteLine($"{corpus.Guards.Count,6} distinct guards  -> {guardPath}");
            Console.WriteLine($"{corpus.Scripts.Count,6} distinct scripts -> {scriptPath}");
            return 0;
        }

        /// <summary>
        /// Writes the database's variable table, so a world can tell a counter from a flag.
        /// </summary>
        /// <remarks>
        /// The one thing the conversation index never carried and every world has had to
        /// guess at. A variable nobody has written reads boolean false - right for the
        /// great majority of guards, and wrong for the 142 counters, whose ordering
        /// comparisons then cannot be evaluated at all. See de-sze.5.4.
        /// </remarks>
        private static int Variables(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string outDir = Option(options, "--out-dir", DefaultDerived);
            RejectUnknownOptions(options);
            Directory.CreateDirectory(outDir);

            IReadOnlyList<DialogueVariable> variables = VariableTableExtractor.Extract(asset);
            string outPath = Path.Combine(outDir, VariableTableFile.FileName);
            VariableTableFile.Write(outPath, variables);

            int numbers = 0;
            foreach (DialogueVariable variable in variables)
            {
                if (variable.Type == "Number")
                {
                    numbers++;
                }
            }

            Console.WriteLine(
                $"wrote {variables.Count} variables ({numbers} numbers, "
                + $"{variables.Count - numbers} other) to {outPath}");
            return 0;
        }

        /// <summary>Writes the database's actor table, so an id can be read as a name.</summary>
        /// <remarks>
        /// What a passive check tests is decided by the ACTOR speaking it - 424 is
        /// Perception (Sight) - and the id is all an entry carries. The game resolves it
        /// through the actor's Articy id; the name is the same answer, and it is the list
        /// the game's own <c>Skill.actorSkillNames</c> is written in, so a reader can map
        /// one to a skill without the database in hand.
        /// </remarks>
        private static int Actors(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string outDir = Option(options, "--out-dir", DefaultDerived);
            RejectUnknownOptions(options);
            Directory.CreateDirectory(outDir);

            IReadOnlyList<DialogueActor> actors = ActorTableExtractor.Extract(asset);
            string outPath = Path.Combine(outDir, ActorTableFile.FileName);
            ActorTableFile.Write(outPath, actors);

            Console.WriteLine($"wrote {actors.Count} actors to {outPath}");
            return 0;
        }

        /// <summary>
        /// Writes the item table with each item's display name, so a save can answer CheckItem.
        /// </summary>
        /// <remarks>
        /// <para>TWO INPUTS, and the only command with two. What a guard names is the item's
        /// ID; what a SAVE records in its key pocket is the English DISPLAY NAME, because that
        /// is what <c>InventoryViewPersister</c> writes - so neither the database nor the save
        /// alone can say which key is held.</para>
        ///
        /// <para>The stack name comes from the database and is what decides which of a save's
        /// three records answers for an item at all - see <see cref="DialogueItem"/>.</para>
        /// </remarks>
        private static int ItemNames(Dictionary<string, string> options)
        {
            string asset = Option(options, "--asset", DefaultAsset);
            string lockit = Option(options, "--lockit", DefaultLockit);
            string outDir = Option(options, "--out-dir", DefaultDerived);
            RejectUnknownOptions(options);
            Directory.CreateDirectory(outDir);

            IReadOnlyList<DialogueItem> items = ItemTableExtractor.Extract(asset);
            IReadOnlyDictionary<string, string> names = ItemNameExtractor.Extract(lockit);

            var named = new List<DialogueItem>(items.Count);
            int unnamed = 0;
            int keys = 0;
            foreach (DialogueItem item in items)
            {
                // AN ITEM WITH NO TERM KEEPS AN EMPTY NAME rather than being left out: its
                // stack name still says how the game answers for it, and only an item on the
                // key ring is ever looked up by name.
                if (names.TryGetValue(item.Name, out string? shown))
                {
                    named.Add(item with { DisplayName = shown });
                }
                else
                {
                    named.Add(item);
                    unnamed++;
                }

                if (item.StackName == "key_ring")
                {
                    keys++;
                }
            }

            string outPath = Path.Combine(outDir, ItemTableFile.FileName);
            ItemTableFile.Write(outPath, named);

            Console.WriteLine(
                $"wrote {named.Count} items ({keys} on the key ring, {unnamed} with no "
                + $"display name) to {outPath}");
            return 0;
        }

        /// <summary>
        /// Writes the index as the mod ships it: everything a crawl reads and nothing else.
        /// </summary>
        /// <remarks>
        /// Reads the full index back rather than re-scanning the 170 MB asset, so the two
        /// cannot disagree about anything but the trimming - which is the only difference
        /// there is meant to be.
        /// </remarks>
        private static int TrimmedIndex(Dictionary<string, string> options)
        {
            string index = Option(options, "--index", DefaultIndex);
            string outPath = Option(options, "--out",
                Path.Combine(DefaultDerived, ShippedIndex.FileName));
            RejectUnknownOptions(options);
            PrepareOutput(outPath);

            int written;
            var trimming = System.Diagnostics.Stopwatch.StartNew();
            using (var writer = new StreamWriter(outPath, append: false, new UTF8Encoding(false)))
            {
                // The header first, because the shipped index is a CACHE and a cache needs
                // to say what it is before it says what is in it.
                writer.WriteLine(ShippedIndex.Header());
                written = ConversationIndexFile.Write(
                    writer, ShippedIndex.Trim(ConversationIndexFile.Read(index)));
            }

            trimming.Stop();

            long before = new FileInfo(index).Length;
            long after = new FileInfo(outPath).Length;
            Console.WriteLine(
                $"wrote {written} conversations to {outPath} "
                + $"({after / 1048576.0:N1} MB, {100.0 * after / before:N0}% of "
                + $"{before / 1048576.0:N1} MB)");

            // Hashing every conversation again, on its own, so the plugin's per-group check
            // can be argued about with a number rather than an estimate. It is only half
            // the answer - the plugin walks a live object graph rather than parsed records -
            // but it bounds the half that is the same on both sides.
            var hashing = System.Diagnostics.Stopwatch.StartNew();
            int hashed = 0;
            foreach (ConversationRecord conversation in ConversationIndexFile.Read(outPath))
            {
                ShippedIndex.HashOf(conversation);
                hashed++;
            }

            hashing.Stop();
            Console.WriteLine(
                $"read, trimmed, hashed and wrote in {trimming.ElapsedMilliseconds:N0} ms; "
                + $"hashing all {hashed} again on its own took "
                + $"{hashing.ElapsedMilliseconds:N0} ms");
            return 0;
        }

        private static int WorstCaseState(Dictionary<string, string> options)
        {
            string index = Option(options, "--index", DefaultIndex);
            string outPath = Option(options, "--out", DefaultStateOut);
            RejectUnknownOptions(options);
            PrepareOutput(outPath);

            SortedDictionary<int, List<int>> displayed =
                WorstCaseGlobalState.Write(outPath, ConversationIndexFile.Read(index));
            int entries = 0;
            foreach (List<int> ids in displayed.Values)
            {
                entries += ids.Count;
            }

            Console.WriteLine($"wrote {displayed.Count} conversations and {entries} entries to {outPath}");
            return 0;
        }

        private static Dictionary<string, string> ParseOptions(string[] args, string command)
        {
            var options = new Dictionary<string, string>(StringComparer.Ordinal);
            for (int i = 1; i < args.Length; i += 2)
            {
                if (i + 1 >= args.Length || !args[i].StartsWith("--", StringComparison.Ordinal))
                {
                    throw new ArgumentException($"Cannot read the options after '{command}'\n\n{Usage}");
                }

                options.Add(args[i], args[i + 1]);
            }

            return options;
        }

        private static void RejectUnknownOptions(Dictionary<string, string> options)
        {
            // Every option a command knows has been removed by now, so whatever is left is
            // one it does not: a misspelling, or an option meant for a different command.
            if (options.Count > 0)
            {
                throw new ArgumentException(
                    $"Unknown option '{FirstKey(options)}'\n\n{Usage}");
            }
        }

        private static void PrepareOutput(string outPath)
        {
            string? directory = Path.GetDirectoryName(outPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }
        }

        private static string Option(Dictionary<string, string> options, string name, string fallback)
        {
            if (!options.Remove(name, out string? value))
            {
                return fallback;
            }

            return value;
        }

        private static string FirstKey(Dictionary<string, string> options)
        {
            foreach (string key in options.Keys)
            {
                return key;
            }

            return string.Empty;
        }
    }
}
