// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Sends a command to the in-game probe by writing the file it watches.
    /// </summary>
    /// <remarks>
    /// <para>The file is written to a temporary name and then moved into place, because
    /// the probe polls with <c>File.Exists</c> and would otherwise be able to read a
    /// half-written command. A move on one volume is atomic, and the temporary name is a
    /// sibling so it is on the same volume by construction.</para>
    ///
    /// <para>Writing the file is not the same as the command having run. The probe
    /// deletes it before executing, so its disappearance only means it was read; what a
    /// caller waits on is the probe event the command produces, through
    /// <see cref="ProbeWatcher"/>.</para>
    /// </remarks>
    public static class ProbeCommand
    {
        /// <summary>The file the probe watches, inside the SaveGames folder.</summary>
        public const string FileName = "gct-probe-command.json";

        /// <summary>Load a savegame by name.</summary>
        public const string LoadSave = "load-save";

        /// <summary>Open a conversation, with no walking and no clicking.</summary>
        public const string StartConversation = "start-conversation";

        /// <summary>Ask the probe to report the state a scenario cares about.</summary>
        public const string Report = "report";

        /// <summary>Evaluate a Lua expression and report what it answered.</summary>
        /// <remarks>
        /// For learning what a guard whose body no export carries actually computes: load a
        /// save that varies what it reads, ask, and write the answer down. The running game
        /// is the only authority on a Final Cut body that was compiled to native code.
        /// </remarks>
        public const string Evaluate = "evaluate";

        /// <summary>
        /// Tell an open conversation to go on to its next line.
        /// </summary>
        /// <remarks>
        /// The game's own continue, called on the dialogue UI, rather than an Enter sent
        /// at the window: a keypress goes wherever the focus is and cannot be aimed at a
        /// line rather than at a menu, which is how a run ends up picking dialogue options
        /// it never meant to.
        /// </remarks>
        public const string Advance = "advance";

        /// <summary>
        /// Advance an open conversation until its response menu is up, and report how many
        /// lines that took.
        /// </summary>
        /// <remarks>
        /// The loop runs inside the game, where the interface can be ASKED what it is
        /// waiting for - the continue button offers a continue, the toggle says when a menu
        /// is up - rather than inferred from the order events reach the log half a second
        /// later. Its answer carries the count, which is a scenario's recorded property.
        /// </remarks>
        public const string AdvanceToMenu = "advance-to-menu";

        /// <summary>
        /// Wait until the open conversation has settled, and say whether a menu is up, a line
        /// is waiting, or the conversation is ending.
        /// </summary>
        /// <remarks>
        /// <see cref="AdvanceToMenu"/> without the advancing, for a run that presses each
        /// continue itself: a scenario's inputs name every one, so a line has to be known to
        /// be waiting - and a menu known not to be arriving beside it - before one is sent.
        /// </remarks>
        public const string Settle = "settle";

        /// <summary>
        /// Choose one option off the response menu that is up, by its destination entry.
        /// </summary>
        /// <remarks>
        /// Through the dialogue UI's own click handler, so what follows is what a player
        /// choosing it would get. Follow it with <see cref="Settle"/> to learn what the option
        /// led to.
        /// </remarks>
        public const string ChooseOption = "choose-option";

        /// <summary>Apply one look-ahead suite's state and runtime settings.</summary>
        public const string PrepareLookAheadSuite = "prepare-look-ahead-suite";

        /// <summary>Flush the current look-ahead suite's diagnostics.</summary>
        public const string FinishLookAheadSuite = "finish-look-ahead-suite";

        /// <summary>
        /// Ask the mod to compare the world it would send the native look-ahead against
        /// the one its managed engine reads.
        /// </summary>
        public const string CheckSnapshot = "check-snapshot";

        /// <summary>
        /// Kill the look-ahead engine, to see what the mod does about it.
        /// </summary>
        /// <remarks>
        /// The one failure the out-of-process engine exists to survive, and a running game
        /// is the only place it can be provoked for real - de-bnjy.1.2.4. The mod does the
        /// killing, because it is the only thing that knows which process is its own.
        /// </remarks>
        public const string KillLookAheadEngine = "kill-look-ahead-engine";

        /// <summary>Asks which process the mod's look-ahead engine is.</summary>
        public const string LookAheadEngineProcess = "look-ahead-engine-process";

        /// <summary>
        /// Press the button on the window the mod raises when its engine dies.
        /// </summary>
        /// <remarks>
        /// What a screenshot cannot answer: a modal that will not close is worse than the
        /// passing notification it replaced, and only pressing it says which kind it is.
        /// </remarks>
        public const string DismissNotice = "dismiss-notice";

        /// <summary>Ask the game to close itself, so the mod can flush on the way out.</summary>
        public const string Quit = "quit";

        /// <summary>Where the command file goes for a given profile.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <exception cref="ArgumentNullException">The folder is null.</exception>
        public static string PathIn(string saveGamesFolder)
        {
            if (saveGamesFolder == null)
            {
                throw new ArgumentNullException(nameof(saveGamesFolder));
            }

            return Path.Combine(saveGamesFolder, FileName);
        }

        /// <summary>Asks the probe to load a savegame.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="saveName">The save's name, without any extension.</param>
        public static void SendLoadSave(string saveGamesFolder, string saveName)
        {
            if (string.IsNullOrWhiteSpace(saveName))
            {
                throw new ArgumentException("A save name is needed.", nameof(saveName));
            }

            Send(saveGamesFolder, LoadSave, "save", saveName);
        }

        /// <summary>Asks the probe to start a conversation by id.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="conversationId">The conversation's database id.</param>
        public static void SendStartConversation(string saveGamesFolder, int conversationId)
        {
            Send(saveGamesFolder, StartConversation, "conversation", conversationId);
        }

        /// <summary>Asks the probe to advance the open conversation by one line.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendAdvance(string saveGamesFolder)
        {
            Send(saveGamesFolder, Advance);
        }

        /// <summary>Asks the probe to advance to the conversation's response menu.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendAdvanceToMenu(string saveGamesFolder)
        {
            Send(saveGamesFolder, AdvanceToMenu);
        }

        /// <summary>Asks the probe what the open conversation is waiting for, once settled.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendSettle(string saveGamesFolder)
        {
            Send(saveGamesFolder, Settle);
        }

        /// <summary>Asks the probe to choose an option off the menu that is up.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="entryId">The option's destination entry.</param>
        public static void SendChooseOption(string saveGamesFolder, int entryId)
        {
            Send(saveGamesFolder, ChooseOption, "entry", entryId);
        }

        /// <summary>Waits until the probe has picked up whatever command is pending.</summary>
        /// <remarks>
        /// THE FILE IS THE SIGNAL, not an event. The probe deletes a command before running
        /// it, so the file going away means it has been taken - which is all a caller
        /// needs before sending the next one, and is knowable without reading the event
        /// log at all. Waiting on the probe's ACKNOWLEDGEMENT instead would mean scanning
        /// the log, and a scan consumes what it passes: the line or menu the command
        /// produced gets swallowed by the wait for the answer that reported it.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="timeout">How long to wait for it to be taken.</param>
        /// <exception cref="TimeoutException">It was never picked up.</exception>
        public static void WaitUntilTaken(string saveGamesFolder, TimeSpan timeout)
        {
            string path = PathIn(saveGamesFolder);
            var clock = Stopwatch.StartNew();
            while (File.Exists(path))
            {
                if (clock.Elapsed >= timeout)
                {
                    throw new TimeoutException(
                        $"The probe did not pick up the command at {path} within "
                        + $"{timeout.TotalSeconds:N0}s.");
                }

                Thread.Sleep(PickUpPoll);
            }
        }

        /// <summary>How often to look for a command having been taken.</summary>
        /// <remarks>
        /// Far shorter than the log poll on purpose: this is a local file check between
        /// two commands that the game answers within a frame or two, and the run advances
        /// one line at a time through it.
        /// </remarks>
        private static readonly TimeSpan PickUpPoll = TimeSpan.FromMilliseconds(50);

        /// <summary>Asks the probe to report where the game currently is.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendReport(string saveGamesFolder)
        {
            Send(saveGamesFolder, Report);
        }

        /// <summary>Asks the probe what a Lua expression answers.</summary>
        /// <remarks>
        /// Answered by an <c>evaluated</c> event carrying <c>read</c> and, when that is
        /// true, <c>value</c>. The two are separate because an expression the game could
        /// not answer must not read as false.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="expression">The expression, without a leading <c>return</c>.</param>
        /// <exception cref="ArgumentException">The expression is blank.</exception>
        public static void SendEvaluate(string saveGamesFolder, string expression)
        {
            if (string.IsNullOrWhiteSpace(expression))
            {
                throw new ArgumentException("An expression is needed.", nameof(expression));
            }

            Send(saveGamesFolder, Evaluate, "expression", expression);
        }

        /// <summary>Asks the mod to prepare one look-ahead suite.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="fileName">A file staged directly in that folder.</param>
        /// <param name="enabled">Whether look-ahead markers are enabled.</param>
        /// <param name="stateBudget">
        /// The most search states one option may hold, or 0 for no such limit. TEST-ONLY:
        /// there is no configuration setting for it, and this command is the only way it
        /// reaches the mod.
        /// </param>
        /// <param name="timeBudgetMs">
        /// The longest one option's crawl may run for, in milliseconds; 0 for no limit.
        /// </param>
        /// <param name="menuTimeBudgetMs">
        /// The longest the whole menu may run for, in milliseconds; 0 for no limit, which is
        /// what every suite asks for. A wall around the menu changes WHICH options give up
        /// rather than making any one of them give up sooner, so it is the wrong instrument
        /// for a suite that wants a crawl starved - see de-dt75.3.
        /// </param>
        /// <param name="memoryBudgetMb">
        /// The most memory one option's crawl may hold, in megabytes; 0 for the engine's
        /// own default. What a suite that wants a crawl to give up sets, now that the
        /// state budget is gone - see de-7z0f.
        /// </param>
        /// <param name="logBudgetExceeded">Whether to log budget overflows.</param>
        /// <param name="keepStatistics">Whether to retain crawl statistics.</param>
        /// <param name="recoveryLimit">
        /// How many engine deaths the mod answers with a fresh engine before giving up for
        /// the session; negative asks for the shipped policy. A SUITE THAT KILLS THE
        /// ENGINE AND EXPECTS THE SHUTDOWN NOTICE MUST SEND ZERO: the shipped limit is
        /// five, so one kill otherwise produces a silent replacement and the notice the
        /// suite is waiting for never comes. See de-bnjy.1.3.
        /// </param>
        /// <param name="keepRequests">
        /// Whether the mod writes out the world each group was crawled from, as the JSON
        /// that crossed to the engine. For comparing a suite against an offline run of it.
        /// </param>
        public static void SendPrepareLookAheadSuite(
            string saveGamesFolder,
            string fileName,
            bool enabled,
            int stateBudget,
            int timeBudgetMs,
            int menuTimeBudgetMs,
            int memoryBudgetMb,
            bool logBudgetExceeded,
            bool keepStatistics,
            int recoveryLimit = -1,
            bool keepRequests = false)
        {
            if (string.IsNullOrWhiteSpace(fileName))
            {
                throw new ArgumentException("A global state file is needed.", nameof(fileName));
            }

            Send(
                saveGamesFolder,
                PrepareLookAheadSuite,
                "file", fileName,
                "enabled", enabled,
                "stateBudget", stateBudget,
                "timeBudgetMs", timeBudgetMs,
                "menuTimeBudgetMs", menuTimeBudgetMs,
                "memoryBudgetMb", memoryBudgetMb,
                "logBudgetExceeded", logBudgetExceeded,
                "keepStatistics", keepStatistics,
                "recoveryLimit", recoveryLimit,
                "keepRequests", keepRequests);
        }

        /// <summary>Asks the mod to flush the current suite's diagnostics.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendFinishLookAheadSuite(string saveGamesFolder)
        {
            Send(saveGamesFolder, FinishLookAheadSuite);
        }

        /// <summary>
        /// Asks the mod to compare its two look-ahead worlds over one conversation group.
        /// </summary>
        /// <remarks>
        /// The command only says the comparison RAN; what it found goes to the BepInEx log,
        /// where <see cref="SnapshotAgreementReport"/> reads it. Needs a loaded save: the
        /// Lua variable table, the dialogue database and the clock all have to exist.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="conversationId">Any conversation in the group to compare over.</param>
        public static void SendCheckSnapshot(string saveGamesFolder, int conversationId)
        {
            Send(saveGamesFolder, CheckSnapshot, "conversation", conversationId);
        }

        /// <summary>Asks the mod to kill its look-ahead engine.</summary>
        /// <remarks>
        /// It stays dead for the rest of the run: the mod does not restart one, which is
        /// the behaviour being tested rather than a limitation of this command.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendKillLookAheadEngine(string saveGamesFolder)
        {
            Send(saveGamesFolder, KillLookAheadEngine);
        }

        /// <summary>Asks which process the mod's look-ahead engine currently is.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <remarks>
        /// Answered by a `look-ahead-engine-process` event carrying `process`, which is 0
        /// while the mod has no engine. A suite that killed one polls this until the id is
        /// non-zero and different, which is the replacement (de-bnjy.1.3).
        /// </remarks>
        public static void SendLookAheadEngineProcess(string saveGamesFolder)
        {
            Send(saveGamesFolder, LookAheadEngineProcess);
        }

        /// <summary>Presses the button on the mod's engine-death window.</summary>
        /// <remarks>
        /// The answer comes back as a <c>notice-dismissed</c> event carrying what the
        /// window looked like before the press and after it, because the press itself
        /// always succeeds - there is a button either way.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendDismissNotice(string saveGamesFolder)
        {
            Send(saveGamesFolder, DismissNotice);
        }

        /// <summary>Asks the game to close itself.</summary>
        /// <remarks>
        /// Killing the process loses everything the mod writes on the way out: the
        /// global state it has not flushed, and its look-ahead statistics, which are
        /// otherwise written only every two hundred crawls.
        /// </remarks>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendQuit(string saveGamesFolder)
        {
            Clear(saveGamesFolder);
            Send(saveGamesFolder, Quit);
        }

        /// <summary>Writes one command.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="command">The command name.</param>
        /// <param name="fields">Alternating name and value.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ProbePendingException">A command is still pending.</exception>
        public static void Send(
            string saveGamesFolder, string command, params object?[] fields)
        {
            if (command == null)
            {
                throw new ArgumentNullException(nameof(command));
            }

            string path = PathIn(saveGamesFolder);
            if (File.Exists(path))
            {
                // The probe deletes a command before running it, so one still sitting
                // there means it has not been picked up - the game is not running, the
                // probe is not loaded, or it is stalled. Overwriting would lose the
                // earlier command and report nothing about why.
                throw new ProbePendingException(
                    $"A probe command is still pending at {path}. The game may not be "
                    + "running, or the probe may not be loaded.");
            }

            Directory.CreateDirectory(saveGamesFolder);

            // Written aside and moved in: the probe polls with File.Exists and would
            // otherwise be able to read a half-written command.
            string staging = path + ".writing";
            File.WriteAllText(staging, Render(command, fields), new UTF8Encoding(false));
            File.Move(staging, path);
        }

        /// <summary>Removes a command nobody picked up.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <returns>Whether there was one to remove.</returns>
        public static bool Clear(string saveGamesFolder)
        {
            string path = PathIn(saveGamesFolder);
            if (!File.Exists(path))
            {
                return false;
            }

            File.Delete(path);
            return true;
        }

        private static string Render(string command, object?[] fields)
        {
            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                writer.WriteString("command", command);

                for (int i = 0; i + 1 < fields.Length; i += 2)
                {
                    object? value = fields[i + 1];
                    if (value == null)
                    {
                        continue;
                    }

                    string key = Convert.ToString(fields[i]) ?? string.Empty;
                    if (value is int number)
                    {
                        writer.WriteNumber(key, number);
                    }
                    else if (value is bool flag)
                    {
                        writer.WriteBoolean(key, flag);
                    }
                    else
                    {
                        writer.WriteString(key, Convert.ToString(value) ?? string.Empty);
                    }
                }

                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }
    }
}
