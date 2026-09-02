// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text.Json;
using PixelCrushers.DialogueSystem;
using UnityEngine;

namespace GlobalConversationTracker.TestProbe
{
    /// <summary>
    /// Carries out the commands a harness leaves in a file, so a run can drive the
    /// game without clicking through its menus.
    /// </summary>
    /// <remarks>
    /// <para>A file rather than a socket or a pipe: the harness and the game are two
    /// processes on one machine with no shared lifetime, one command is in flight at a
    /// time, and a file is the only channel that needs no handshake, survives the game
    /// not having started yet, and can be read afterwards when something went
    /// wrong.</para>
    ///
    /// <para>Each command is consumed by deleting the file before it runs, so a command
    /// cannot be executed twice if it throws or if the game stalls. The outcome is
    /// reported as a probe event, which is what the harness actually waits on - the file
    /// disappearing only means it was read.</para>
    ///
    /// <para>What this replaces is menu navigation. Loading a save through the game's
    /// own loader reaches the same <c>PersistentDataManager.ApplyRawData</c> that a
    /// menu click reaches, and starting a conversation through the dialogue system runs
    /// the identical code that composes an option's text - so nothing under test is
    /// lost, and the part that could only be learned from a screenshot is gone.</para>
    /// </remarks>
    internal sealed class ProbeCommands : MonoBehaviour
    {
        /// <summary>The file the harness writes a command into.</summary>
        internal const string CommandFileName = "gct-probe-command.json";

        /// <summary>Load a savegame by name.</summary>
        internal const string LoadSaveCommand = "load-save";

        /// <summary>Open a conversation by id, with no walking and no clicking.</summary>
        internal const string StartConversationCommand = "start-conversation";

        /// <summary>Report the state a scenario cares about.</summary>
        internal const string ReportCommand = "report";

        /// <summary>Replace the mod's global state from a staged fixture.</summary>
        internal const string PrepareLookAheadSuiteCommand = "prepare-look-ahead-suite";
        internal const string FinishLookAheadSuiteCommand = "finish-look-ahead-suite";

        /// <summary>Ask the game to close itself the way a player would.</summary>
        internal const string QuitCommand = "quit";

        /// <summary>
        /// How many frames pass between checks. The harness waits on a probe event
        /// rather than on a deadline, so this only decides how quickly a command is
        /// noticed; a per-frame File.Exists on a path that is usually absent is cheap,
        /// but not free, and nothing here needs frame accuracy.
        /// </summary>
        private const int PollFrames = 10;

        private static string? _commandPath;
        private int _sinceLastPoll;
        private bool _wasLoading;

        /// <summary>Required by Il2CppInterop for an injected component.</summary>
        /// <param name="pointer">The native object.</param>
        public ProbeCommands(IntPtr pointer)
            : base(pointer)
        {
        }

        /// <summary>Where commands are read from.</summary>
        /// <remarks>
        /// Beside the global state in the SaveGames folder, which the harness already
        /// stages and already reads artefacts out of. It is not the game install, so a
        /// command file left behind by a killed run cannot outlive the staged profile.
        /// </remarks>
        internal static string CommandPath => _commandPath ?? string.Empty;

        /// <summary>Points the pump at a directory. Call before the component runs.</summary>
        /// <param name="directoryPath">The SaveGames folder.</param>
        internal static void UseDirectory(string directoryPath)
        {
            _commandPath = Path.Combine(directoryPath, CommandFileName);
        }

        /// <summary>Polls for a command. Called by Unity.</summary>
        public void Update()
        {
            if (++_sinceLastPoll < PollFrames)
            {
                return;
            }

            _sinceLastPoll = 0;
            ReportLoadingFinished();

            string path = CommandPath;
            if (path.Length == 0 || !File.Exists(path))
            {
                return;
            }

            string text;
            try
            {
                text = File.ReadAllText(path);
                // Consumed before it runs: a command that throws, or that stalls the
                // game, must not be picked up again on the next poll.
                File.Delete(path);
            }
            catch (IOException)
            {
                // Half-written, or still open in the harness. Try again next poll.
                return;
            }

            Run(text);
        }

        /// <summary>
        /// Says when a load has actually finished, which nothing else does.
        /// </summary>
        /// <remarks>
        /// <c>save-applied</c> fires while the save is still being applied, and
        /// <c>world-ready</c> fires when the HUD is first built - once, at the main menu
        /// - so neither marks the moment the loaded world is there. The game's own
        /// <c>IsLoading</c> flag does, and watching it fall is the only signal that
        /// survives loading a second save into a session that already has a HUD.
        /// </remarks>
        private void ReportLoadingFinished()
        {
            bool loading;
            try
            {
                SunshinePersistence? persistence = SunshinePersistence.Singleton;
                loading = persistence != null && persistence.IsLoading;
            }
            catch (Exception)
            {
                return;
            }

            if (_wasLoading && !loading)
            {
                ProbeLog.Write(
                    "load-finished",
                    "money", TestProbePlugin.Money(),
                    "conversation", TestProbePlugin.ConversationId());
            }

            _wasLoading = loading;
        }

        private static void Run(string text)
        {
            string name = "?";
            try
            {
                using JsonDocument document = JsonDocument.Parse(text);
                JsonElement root = document.RootElement;
                name = Member(root, "command") ?? "?";

                switch (name)
                {
                    case LoadSaveCommand:
                        LoadSave(Member(root, "save"));
                        break;
                    case StartConversationCommand:
                        StartConversation(root);
                        break;
                    case PrepareLookAheadSuiteCommand:
                        PrepareLookAheadSuite(root);
                        break;
                    case FinishLookAheadSuiteCommand:
                        InvokePlugin("FinishLookAheadSuite", Array.Empty<object>());
                        ProbeLog.Write("look-ahead-suite-finished");
                        break;
                    case QuitCommand:
                        // Not a kill. The mod flushes its global state and writes its
                        // look-ahead statistics from Application.quitting, so a run that
                        // killed the process would lose both - and the statistics file is
                        // otherwise only written every two hundred crawls, far more than
                        // a scenario produces.
                        ProbeLog.Write("command-started", "command", QuitCommand);
                        Application.Quit();
                        break;
                    case ReportCommand:
                        ProbeLog.Write(
                            "report",
                            "money", TestProbePlugin.Money(),
                            "conversation", TestProbePlugin.ConversationId());
                        break;
                    default:
                        ProbeLog.Write(
                            "command-failed", "command", name, "message", "unknown command");
                        break;
                }
            }
            catch (Exception error)
            {
                ProbeLog.Write(
                    "command-failed", "command", name, "message", Explain(error));
            }
        }

        private static string? Member(JsonElement root, string name)
        {
            return root.ValueKind == JsonValueKind.Object
                && root.TryGetProperty(name, out JsonElement value)
                && value.ValueKind == JsonValueKind.String
                ? value.GetString()
                : null;
        }

        private static int? NumberMember(JsonElement root, string name)
        {
            return root.ValueKind == JsonValueKind.Object
                && root.TryGetProperty(name, out JsonElement value)
                && value.ValueKind == JsonValueKind.Number
                && value.TryGetInt32(out int number)
                ? number
                : (int?)null;
        }

        private static bool? BoolMember(JsonElement root, string name)
        {
            return root.ValueKind == JsonValueKind.Object
                && root.TryGetProperty(name, out JsonElement value)
                && (value.ValueKind == JsonValueKind.True
                    || value.ValueKind == JsonValueKind.False)
                ? value.GetBoolean()
                : (bool?)null;
        }

        private static void LoadSave(string? save)
        {
            if (string.IsNullOrEmpty(save))
            {
                throw new ArgumentException("No save name was given.");
            }

            ProbeLog.Write("command-started", "command", LoadSaveCommand, "save", save);

            // Not SunshinePersistence.CanLoad(): it reads ViewsPagesBridge.Current, which
            // is null until the menu exists, so asking merely to log the answer threw and
            // failed the command it was describing. The singleton being there is the only
            // precondition worth checking, and the caller waits for the game's UI anyway.
            SunshinePersistence persistence = SunshinePersistence.Singleton
                ?? throw new InvalidOperationException(
                    "SunshinePersistence has no instance yet; the game is still starting.");

            // The same call the Load Game menu item makes, so the save travels the path
            // the tests already hook rather than a private shortcut. Not bundled: these
            // are ordinary saves staged into the profile's SaveGames folder.
            persistence.Load(save!, false);
        }

        private static void PrepareLookAheadSuite(JsonElement root)
        {
            string? fileName = Member(root, "file");
            if (string.IsNullOrWhiteSpace(fileName)
                || !string.Equals(fileName, Path.GetFileName(fileName), StringComparison.Ordinal))
            {
                throw new ArgumentException(
                    "Give a global state filename directly inside SaveGames.");
            }

            string directory = Path.GetDirectoryName(CommandPath)
                ?? throw new InvalidOperationException("The probe command directory is unavailable.");
            string sourcePath = Path.Combine(directory, fileName);

            bool enabled = BoolMember(root, "enabled")
                ?? throw new ArgumentException("No enabled setting was given.");
            int stateBudget = NumberMember(root, "stateBudget")
                ?? throw new ArgumentException("No state budget was given.");
            bool logBudgetExceeded = BoolMember(root, "logBudgetExceeded")
                ?? throw new ArgumentException("No budget-log setting was given.");
            bool keepStatistics = BoolMember(root, "keepStatistics")
                ?? throw new ArgumentException("No statistics setting was given.");

            ProbeLog.Write(
                "command-started",
                "command", PrepareLookAheadSuiteCommand,
                "file", fileName);
            InvokePlugin(
                "PrepareLookAheadSuite",
                new object[]
                {
                    sourcePath, enabled, stateBudget, logBudgetExceeded, keepStatistics,
                });
            ProbeLog.Write(
                "look-ahead-suite-prepared",
                "file", fileName,
                "enabled", enabled,
                "stateBudget", stateBudget,
                "logBudgetExceeded", logBudgetExceeded,
                "keepStatistics", keepStatistics);
        }

        /// <summary>
        /// The message to report for a failed command, following the chain of causes.
        /// </summary>
        /// <remarks>
        /// Everything here reaches the plugin by reflection, and a method that throws
        /// comes back wrapped in a TargetInvocationException whose own message is the
        /// useless "Exception has been thrown by the target of an invocation." Reporting
        /// that alone says a command failed and nothing whatever about why - which turned
        /// a plugin refusing a file format it did not recognise into an unexplained hang.
        /// </remarks>
        private static string Explain(Exception error)
        {
            var parts = new List<string>();
            for (Exception? cause = error; cause != null; cause = cause.InnerException)
            {
                // The wrapper's own message carries nothing the cause does not.
                if (cause is TargetInvocationException && cause.InnerException != null)
                {
                    continue;
                }

                parts.Add($"{cause.GetType().Name}: {cause.Message}");
            }

            return string.Join(" -> ", parts);
        }

        private static void InvokePlugin(string methodName, object[] arguments)
        {
            Type plugin = Type.GetType(
                    "GlobalConversationTracker.GlobalConversationTrackerPlugin, "
                        + "GlobalConversationTracker",
                    throwOnError: true)
                ?? throw new InvalidOperationException(
                    "The Global Conversation Tracker plugin assembly is not loaded.");
            MethodInfo method = plugin.GetMethod(
                    methodName,
                    BindingFlags.Public | BindingFlags.Static)
                ?? throw new MissingMethodException(plugin.FullName, methodName);
            method.Invoke(null, arguments);
        }

        private static void StartConversation(JsonElement root)
        {
            int? conversationId = NumberMember(root, "conversation");
            string? title = Member(root, "title");
            if (conversationId == null && string.IsNullOrEmpty(title))
            {
                throw new ArgumentException(
                    "Give either a conversation id or a title to start.");
            }

            string resolved = title ?? TitleOf(conversationId!.Value);
            ProbeLog.Write(
                "command-started",
                "command", StartConversationCommand,
                "conversation", conversationId,
                "title", resolved);

            DialogueManager.StartConversation(resolved);

            // Whether it took is not obvious from the call: StartConversation returns
            // nothing and a conversation whose first node is gated simply ends. Saying
            // what happened here is the difference between "the id was wrong" and "the
            // conversation started and had nothing to offer".
            ProbeLog.Write(
                "command-finished",
                "command", StartConversationCommand,
                "title", resolved,
                "active", DialogueManager.isConversationActive,
                "conversation", TestProbePlugin.ConversationId());
        }

        private static string TitleOf(int conversationId)
        {
            DialogueDatabase database = DialogueManager.masterDatabase
                ?? throw new InvalidOperationException(
                    "There is no dialogue database yet; load a save first.");

            Conversation conversation = database.GetConversation(conversationId)
                ?? throw new InvalidOperationException(
                    $"No conversation {conversationId} in the database.");

            return conversation.Title;
        }
    }
}
