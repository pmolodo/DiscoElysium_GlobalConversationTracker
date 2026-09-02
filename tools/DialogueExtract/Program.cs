// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using GlobalConversationTracker.DialogueAsset;

namespace GlobalConversationTracker.DialogueExtract
{
    /// <summary>Reads what the offline tools need out of the Dialogue System database.</summary>
    internal static class Program
    {
        private const string ConversationIndexCommand = "conversation-index";
        private const int ExitFailure = 1;

        private const string Usage = """
            DialogueExtract - read a Disco Elysium Dialogue System database .asset.

            Usage:
              dotnet run --project tools/DialogueExtract -- conversation-index [options]

            Commands:
              conversation-index  One compact JSON object per conversation, one per line:
                                  its id, title, actor, conversant, and every dialogue
                                  entry with its guard, script, links and fields.

            Options:
              --asset PATH  The database .asset. Default:
                            .game_reference_copies/AssetRipperExport/ExportedProject/Assets/Dialogue Databases/Disco Elysium.asset
              --out PATH    Where to write the output. Default:
                            .game_reference_copies/derived/conversation_index.jsonl
              -h, --help    Show this message.
            """;

        private static readonly string DefaultAsset = Path.Combine(".game_reference_copies", "AssetRipperExport",
            "ExportedProject", "Assets", "Dialogue Databases", "Disco Elysium.asset");

        private static readonly string DefaultOut = Path.Combine(".game_reference_copies", "derived",
            "conversation_index.jsonl");

        private static int Main(string[] args)
        {
            try
            {
                return Run(args);
            }
            catch (Exception exception) when (exception is ArgumentException or IOException)
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
            if (command != ConversationIndexCommand)
            {
                throw new ArgumentException($"Unknown command '{command}'\n\n{Usage}");
            }

            var options = new Dictionary<string, string>(StringComparer.Ordinal);
            for (int i = 1; i < args.Length; i += 2)
            {
                if (i + 1 >= args.Length || !args[i].StartsWith("--", StringComparison.Ordinal))
                {
                    throw new ArgumentException($"Cannot read the options after '{command}'\n\n{Usage}");
                }

                options.Add(args[i], args[i + 1]);
            }

            string asset = Option(options, "--asset", DefaultAsset);
            string outPath = Option(options, "--out", DefaultOut);
            if (options.Count > 0)
            {
                throw new ArgumentException(
                    $"Unknown option '{FirstKey(options)}'\n\n{Usage}");
            }

            string? directory = Path.GetDirectoryName(outPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            int written = ConversationIndexFile.Write(outPath, ConversationIndexExtractor.Extract(asset));
            Console.WriteLine($"wrote {written} conversations to {outPath}");
            return 0;
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
