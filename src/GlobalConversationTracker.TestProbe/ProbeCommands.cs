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

        /// <summary>
        /// Ask the mod to compare the world it would send the native engine against the
        /// one its managed engine reads.
        /// </summary>
        /// <remarks>
        /// The answer goes to the BepInEx log rather than to a probe event, because the
        /// comparison happens inside the PLUGIN - it is the plugin's two worlds being
        /// compared - and reaching it from here already costs a reflection call. A probe
        /// event would mean marshalling the report back through that call to write it out
        /// again beside a line the plugin can write itself.
        /// </remarks>
        internal const string CheckSnapshotCommand = "check-snapshot";

        /// <summary>Ask the game to close itself the way a player would.</summary>
        internal const string QuitCommand = "quit";

        /// <summary>
        /// How many frames pass between checks. The harness waits on a probe event
        /// rather than on a deadline, so this only decides how quickly a command is
        /// noticed; a per-frame File.Exists on a path that is usually absent is cheap,
        /// but not free, and nothing here needs frame accuracy.
        /// </summary>
        private const int PollFrames = 10;

        /// <summary>
        /// How many polls may pass after a load is latched before "not loading" is
        /// taken to mean the load is over.
        /// </summary>
        /// <remarks>
        /// Only ever reached when the rising edge was missed. Watching for the falling
        /// edge alone waits forever on a load whose <c>IsLoading</c> window opens and
        /// closes between two polls, because such a load never looks like it started.
        /// Long enough that a latch taken just before <c>IsLoading</c> rises does not
        /// report the load before it has begun, short enough that the settle costs a
        /// genuinely fast load nothing worth measuring.
        /// </remarks>
        private const int LoadSettlePolls = 3;

        private static string? _commandPath;
        private static bool _saveApplied;
        private int _sinceLastPoll;
        private bool _loadPending;
        private bool _sawLoading;
        private int _pollsSinceLoad;

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

        /// <summary>
        /// Says that a save is being applied, so the load that follows is reported even
        /// if it finishes too quickly to be observed.
        /// </summary>
        /// <remarks>
        /// Called from the <c>ApplyRawData</c> hook, which is the one place every load
        /// goes through, whether it came from the main menu or from a probe command.
        /// Latching there rather than on seeing <c>IsLoading</c> rise is what makes the
        /// report survive a load that opens and closes between two polls.
        /// </remarks>
        internal static void NoteSaveApplied()
        {
            _saveApplied = true;
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
        /// <para><c>save-applied</c> fires while the save is still being applied, and
        /// <c>world-ready</c> fires when the HUD is first built - once, at the main menu
        /// - so neither marks the moment the loaded world is there. The game's own
        /// <c>IsLoading</c> flag does, and watching it fall is the only signal that
        /// survives loading a second save into a session that already has a HUD.</para>
        ///
        /// <para>But the fall can only be watched for once the rise has been seen, and
        /// at one poll every ten frames a fast load can begin and end unobserved. Then
        /// the falling edge never fires, and a harness waiting on it waits out its whole
        /// timeout against a game that finished the load and is sitting there idle -
        /// which is what it looks like from outside, and it is not what happened. So the
        /// load is latched when the save is applied instead. Seeing the rise still
        /// reports the exact falling edge, as before; not seeing it now reports the load
        /// a few polls late rather than never.</para>
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
                // A singleton that throws mid-scene-swap costs this poll, not the latch:
                // a pending load stays pending and the next poll asks again.
                return;
            }

            // Watched on every poll, not only while a load is pending, because the flag
            // rises BEFORE the save is applied - measured: five consecutive loads all
            // reported the rise as unseen when the watch began at the latch. Keeping the
            // watch running is what lets the falling edge stay the signal, with the
            // latch's settle as the fallback rather than the usual path.
            if (loading)
            {
                _sawLoading = true;
            }

            if (_saveApplied)
            {
                _saveApplied = false;
                _loadPending = true;
                _pollsSinceLoad = 0;
            }

            if (!_loadPending)
            {
                return;
            }

            _pollsSinceLoad++;
            if (loading || (!_sawLoading && _pollsSinceLoad < LoadSettlePolls))
            {
                return;
            }

            _loadPending = false;
            bool observed = _sawLoading;
            _sawLoading = false;
            ProbeLog.Write(
                "load-finished",
                "money", TestProbePlugin.Money(),
                "conversation", TestProbePlugin.ConversationId(),
                "observed", observed);
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
                    case CheckSnapshotCommand:
                        CheckSnapshot(root);
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
                            "conversation", TestProbePlugin.ConversationId(),
                            "active", TestProbePlugin.IsConversationActive());
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

        /// <summary>Asks the mod to compare its two worlds over one conversation group.</summary>
        private static void CheckSnapshot(JsonElement root)
        {
            int conversation = NumberMember(root, "conversation")
                ?? throw new ArgumentException("No conversation was given.");

            ProbeLog.Write(
                "command-started",
                "command", CheckSnapshotCommand,
                "conversation", conversation);
            InvokePlugin("CheckLookAheadSnapshot", new object[] { conversation });
            // Says the comparison RAN. What it found is in the BepInEx log, which is where
            // the plugin wrote it and where the harness reads it from.
            ProbeLog.Write("snapshot-checked", "conversation", conversation);
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
            // Absent means no limit, so an older harness that does not send it keeps
            // working and gets the behaviour it had.
            int timeBudgetMs = NumberMember(root, "timeBudgetMs") ?? 0;
            // Absent means the engine's own default rather than no limit, which is what
            // zero means for this one - see de-e23q. A suite that does not mention a memory
            // budget gets the shipped behaviour, which is what not mentioning it means.
            int memoryBudgetMb = NumberMember(root, "memoryBudgetMb") ?? 0;
            bool logBudgetExceeded = BoolMember(root, "logBudgetExceeded")
                ?? throw new ArgumentException("No budget-log setting was given.");
            bool keepStatistics = BoolMember(root, "keepStatistics")
                ?? throw new ArgumentException("No statistics setting was given.");

            ProbeLog.Write(
                "command-started",
                "command", PrepareLookAheadSuiteCommand,
                "file", fileName);
            // EVERY PARAMETER, INCLUDING THE OPTIONAL ONE. This is a reflection call, and
            // reflection does not fill in a C# default: a method that grew an optional
            // parameter and a probe that kept passing the old count is a
            // TargetParameterCountException at run time, which is a whole in-game run lost
            // to a mismatch the compiler never sees.
            InvokePlugin(
                "PrepareLookAheadSuite",
                new object[]
                {
                    sourcePath, enabled, stateBudget, timeBudgetMs, logBudgetExceeded,
                    keepStatistics, memoryBudgetMb,
                });
            ProbeLog.Write(
                "look-ahead-suite-prepared",
                "file", fileName,
                "enabled", enabled,
                "stateBudget", stateBudget,
                "timeBudgetMs", timeBudgetMs,
                "memoryBudgetMb", memoryBudgetMb,
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
                "active", TestProbePlugin.IsConversationActive(),
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
