// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// Asks a running game what an expression answers, once per save, and writes the
    /// answers down.
    /// </summary>
    /// <remarks>
    /// <para>WHAT IT IS FOR. Most of what Final Cut ships has no readable body: that build
    /// is IL2CPP, and the export stubs every method it could not reconstruct. What such a
    /// function COMPUTES can then only be learned by asking a running game. Vary the state
    /// it reads across a set of saves, evaluate it in each, and the answers are a truth
    /// table for a function whose source is gone.</para>
    ///
    /// <para>WHY IT IS NOT THE LOOK-AHEAD RUN. That one opens a conversation and reads the
    /// markers off the menu the game drew, so what it can observe is whatever queries the
    /// conversation's group happens to make. This asks outright, which is both narrower and
    /// far more direct - no conversation is opened at all.</para>
    ///
    /// <para>IT ALSO ASSERTS MUCH LESS about the machine. The look-ahead run refuses to
    /// continue unless the window opened at the requested size, because every screen
    /// reference it compares against was captured there; nothing here is measured in
    /// pixels, so a wrong-sized window costs nothing and is not made into a failure.</para>
    ///
    /// <para>## THE CONTROL, WHICH IS THE WHOLE DIFFERENCE BETWEEN DATA AND WISHES</para>
    ///
    /// <para>A load that does not take is the failure this instrument is most exposed to. A
    /// save may carry a combination the game's own loader refuses - its restore path may
    /// branch on one field and leave another alone - and then the expression is answered
    /// about whatever was in BEFORE it, while looking exactly like a row of the table.
    /// Every such row would be plausible, and the table would be wrong in a way nothing in
    /// it reported.</para>
    ///
    /// <para><see cref="Run"/> takes CONTROL saves for that. Each test save is loaded after
    /// a control, and the expressions asked of both. A stale answer is the CONTROL'S
    /// answer - so if the test's answer DIFFERS from the control's, the state demonstrably
    /// changed and the load took. If they agree, the row is ambiguous and the next control
    /// is tried.</para>
    ///
    /// <para>WHICH MAKES MOST ROWS COST ONE CONTROL RATHER THAN ALL OF THEM. A row that
    /// diverges is settled by that divergence and stops there; only a row that agrees with
    /// its control has anything left to find out. Control ORDER therefore matters: putting
    /// the control that disagrees with most test saves first is what makes the run short.
    /// </para>
    ///
    /// <para>AND EXHAUSTING THE CONTROLS IS AN ANSWER, not a shrug. Given two controls that
    /// answer differently FROM EACH OTHER, no honest save can agree with both - so a test
    /// save that never diverged did not load, and the run says so rather than reporting its
    /// answer as a row.</para>
    ///
    /// <para>THE CONTROL IS ALSO ASKED, immediately after it loads and before the test save
    /// goes in. Without that, two controls that both failed to load would agree with each
    /// other and with everything else, and the agreement would read as the save deciding -
    /// the exact mistake the control exists to catch, one level up.</para>
    ///
    /// <para>WHAT A CONTROL SHOULD ANSWER IS NOT WRITTEN DOWN HERE. Divergence is compared
    /// between answers this run observed; nothing tells it what any save is expected to
    /// say. An instrument that knew the expected answer could confirm it.</para>
    ///
    /// <para>## A GUESS IS ALLOWED, AS A SCHEDULE AND NOT AS EVIDENCE</para>
    ///
    /// <para>A caller may say which control to try FIRST for each save, predicted from
    /// whatever it thinks the function does - the useful prediction being the control that
    /// disagrees with the guess, since that is the one that diverges at once. This is the
    /// only place a hypothesis is allowed in, and it is allowed because it cannot reach the
    /// answer: it decides the ORDER controls are tried in, nothing else. A row is still
    /// settled by a divergence this run observed, and a wrong prediction costs the second
    /// control rather than a wrong row.</para>
    ///
    /// <para>Nor can it be fooled by the failure it exists to catch. Predict false, load the
    /// control that answers true: if the test save does not load, it answers true as well -
    /// agreeing with the control, which is not a divergence, so the row goes on to the next
    /// control exactly as it should.</para>
    /// </remarks>
    internal static class EvaluateRun
    {
        /// <summary>Where the answers are written, under the artifacts folder.</summary>
        private const string AnswersFileName = "evaluate-answers.json";

        /// <summary>What an expanded save directory is called.</summary>
        private const string ExpandedSuffix = ".ntwtf";

        /// <summary>
        /// What the first row followed, since nothing this run loaded came before it.
        /// </summary>
        private const string LoadedByContinue = "(continue)";

        /// <summary>A row asked to establish what came before a test row.</summary>
        private const string ControlRole = "control";

        /// <summary>A row the table is actually about.</summary>
        private const string TestRole = "test";

        /// <summary>How long to let the world settle after a load reports finished.</summary>
        /// <remarks>
        /// <para>LOAD-FINISHED IS NOT THE WORLD BEING READY, and everything asked here is
        /// asked THROUGH Lua. The look-ahead run documents the same gap - between the
        /// game's loading flag falling and the loaded save's state reaching Lua - and waits
        /// five seconds before asking a conversation anything for exactly this reason. An
        /// evaluate sent into that gap answers about the save BEFORE this one.</para>
        ///
        /// <para>IT ALSO SPACES CONSECUTIVE LOADS, which matters at least as much.
        /// Measured without it: a load issued while the previous one was still settling did
        /// nothing whatsoever - reporting finished in 0.1s against the 2.5s a real load
        /// takes, and leaving every answer as the previous save's. The control caught every
        /// one of those rows, which is the only reason the run was not quietly believed.
        /// </para>
        /// </remarks>
        private static readonly TimeSpan AfterLoad = TimeSpan.FromSeconds(5);

        /// <summary>Evaluates every expression against every save, in one launch.</summary>
        /// <param name="game">Path to disco.exe.</param>
        /// <param name="savesRoot">A folder of expanded saves to load in turn.</param>
        /// <param name="settingsFile">The test settings to stage.</param>
        /// <param name="artifacts">Where packed saves and the answers go.</param>
        /// <param name="timeout">How long any single wait may take.</param>
        /// <param name="expressions">The expressions to ask of each save.</param>
        /// <param name="controls">
        /// Saves to load before a test save, so a load that did not take can be told from
        /// one that did. Tried in the order given, and only until one of them makes the
        /// test save's answer differ. Empty for a plain pass with no control at all.
        /// </param>
        /// <param name="firstControl">
        /// Which control to try first for a given save, predicted by the caller, or null to
        /// take them in the order given. A SCHEDULE AND NOT EVIDENCE - see the remarks.
        /// </param>
        /// <param name="keepOpen">Leave the game running, restoring nothing.</param>
        /// <param name="backupProfile">
        /// How to move the player's profile aside, or null for the plain move.
        /// </param>
        /// <returns>0 when every row was answered and, where controlled, resolved.</returns>
        public static int Run(
            string game,
            string savesRoot,
            string settingsFile,
            string artifacts,
            TimeSpan timeout,
            IReadOnlyList<string> expressions,
            IReadOnlyList<string> controls,
            IReadOnlyDictionary<string, string>? firstControl,
            bool keepOpen,
            Func<string, ProfileBackup>? backupProfile = null)
        {
            if (expressions == null || expressions.Count == 0)
            {
                throw new ArgumentException(
                    "Nothing to ask. Name at least one expression with --expression.",
                    nameof(expressions));
            }

            controls ??= Array.Empty<string>();

            if (!Directory.Exists(savesRoot))
            {
                throw new DirectoryNotFoundException($"No saves folder at {savesRoot}.");
            }

            string[] expanded = Directory
                .GetDirectories(savesRoot, "*" + ExpandedSuffix)
                .OrderBy(path => Path.GetFileName(path), StringComparer.Ordinal)
                .ToArray();

            if (expanded.Length == 0)
            {
                throw new InvalidOperationException(
                    $"{savesRoot} holds no expanded saves. Write some first - see "
                    + "tools/make-party-saves.py.");
            }

            List<string> names = expanded
                .Select(Path.GetFileName)
                .Select(name => name!.Substring(0, name!.Length - ExpandedSuffix.Length))
                .ToList();

            // BEFORE ANYTHING IS PACKED. A control that is a typo names no save, and every
            // row of the run would then silently have no control at all - so it is refused
            // here rather than after thirty-two saves have been packed for nothing.
            foreach (string control in controls)
            {
                if (!names.Contains(control, StringComparer.Ordinal))
                {
                    throw new ArgumentException(
                        $"No save called '{control}' in {savesRoot}. It holds: "
                        + string.Join(", ", names),
                        nameof(controls));
                }
            }

            // Refused rather than ignored. A prediction naming a control that is not in the
            // run would quietly do nothing, and the run would be slower than asked for with
            // nothing saying why.
            if (firstControl != null)
            {
                foreach (KeyValuePair<string, string> predicted in firstControl)
                {
                    if (!controls.Contains(predicted.Value, StringComparer.Ordinal))
                    {
                        throw new ArgumentException(
                            $"The prediction for '{predicted.Key}' names "
                            + $"'{predicted.Value}', which is not one of this run's "
                            + $"controls: {string.Join(", ", controls)}",
                            nameof(firstControl));
                    }
                }
            }

            Console.WriteLine($"game:      {game}");
            Console.WriteLine($"saves:     {expanded.Length} in {savesRoot}");
            Console.WriteLine($"asking:    {string.Join(", ", expressions)}");
            Console.WriteLine(
                controls.Count == 0
                    ? "controls:  none, so a load that did not take cannot be told from one "
                        + "that did"
                    : $"controls:  {string.Join(", ", controls)}, in that order, until one "
                        + "of them makes the answer differ");
            Directory.CreateDirectory(artifacts);

            // PACKED IN ORDER, so the LAST one is the newest on disk and is therefore what
            // Continue loads. Which save that is does not matter - every save is loaded
            // deliberately by name afterwards - but WHICH ONE IT WAS does, because it is
            // what the first row followed.
            var stagedNames = new Dictionary<string, string>(StringComparer.Ordinal);
            var packed = new List<string>();
            foreach (string directory in expanded)
            {
                string name = Path.GetFileName(directory);
                name = name.Substring(0, name.Length - ExpandedSuffix.Length);
                string archive = Program.PackSave(directory, artifacts);
                packed.Add(archive);

                // The packer stamps the time into the archive's name and the game keys a
                // save by exactly that, so a load command has to carry the staged name and
                // not the folder's. Getting this wrong fails silently: the load is refused
                // and the answers describe whatever was already in.
                string fileName = Path.GetFileName(archive);
                stagedNames[name] =
                    fileName.EndsWith(GameSaves.SaveExtension, StringComparison.OrdinalIgnoreCase)
                        ? fileName.Substring(
                            0, fileName.Length - GameSaves.SaveExtension.Length)
                        : fileName;
            }

            var report = new Report();
            var answers = new List<Answer>();
            string logPath = Path.Combine(
                FilePaths.FolderOf(game, nameof(game)), "BepInEx", "LogOutput.log");
            string saveGames = GameProfile.SavesFolder;
            Process? process = null;

            using StagedGame staged = StagedGame.Stage(
                "disco",
                settingsFile,
                packed,
                globalStateFile: null,
                backupProfile: backupProfile,
                keepPlayerLogAt: Program.PlayerLogPath("evaluate"),
                progress: message => Console.WriteLine($"staging:   {message}"));

            using ProbeDeployment probe = ProbeDeployment.Deploy(
                game,
                GameInstall.FindProbeAssembly(),
                message => Console.WriteLine($"probe:     {message}"));

            try
            {
                // Before launching, not after. BepInEx truncates its log when it starts,
                // but this begins reading the instant the process exists, and in that gap
                // it would find the previous run's events.
                if (File.Exists(logPath))
                {
                    File.Delete(logPath);
                }

                Console.WriteLine("launching...");
                process = Process.Start(
                    new ProcessStartInfo(game) { UseShellExecute = false });

                var watcher = new ProbeWatcher(logPath);

                // Every wait from here gives up the moment the game is gone: it will not
                // report anything again, and the profile is staged until this returns.
                Process? launched = process;
                watcher.AbandonIf(
                    () => launched != null && launched.HasExited,
                    () => $"the game is no longer running, {GameExit.Describe(launched)}");

                watcher.WaitForEvent("ready", timeout, Log);
                report.Check(true, "the probe loaded", $"reading {logPath}");

                // THE FIRST SAVE COMES IN THROUGH THE MENU'S CONTINUE, not a load by name -
                // see FirstSave - and only then can the rest be loaded by name.
                FirstSave.Get(watcher, timeout, saveGames);

                string previous = LoadedByContinue;

                List<Answer> Ask(string save, string role)
                {
                    Console.WriteLine();
                    Console.WriteLine($"=== {save} ({role}, after {previous}) ===");

                    watcher.Mark();
                    ProbeCommand.SendLoadSave(saveGames, stagedNames[save]);
                    watcher.WaitForEvent("load-finished", timeout, Log);

                    // AND THEN WAIT. See AfterLoad: the loading flag falling is not the
                    // loaded state having reached Lua, and everything below is asked
                    // through Lua.
                    Thread.Sleep(AfterLoad);

                    var asked = new List<Answer>();
                    foreach (string expression in expressions)
                    {
                        watcher.Mark();
                        ProbeCommand.SendEvaluate(saveGames, expression);

                        // MATCHED ON THE EXPRESSION, not just on the event name. Several
                        // are asked per save, and an answer taken for the wrong one would
                        // be a plausible row rather than an obvious failure.
                        ProbeEvent answered = watcher.WaitFor(
                            e => e.Name == "evaluated"
                                && e.Text("expression") == expression,
                            timeout,
                            $"an answer to {expression}",
                            Log);

                        var answer = new Answer(save, previous, role, expression, answered);
                        asked.Add(answer);
                        answers.Add(answer);

                        report.Check(
                            answer.Read,
                            $"{save} ({role}): {expression} answered",
                            answer.Describe());
                    }

                    previous = save;
                    return asked;
                }

                foreach (string name in names)
                {
                    if (controls.Count == 0)
                    {
                        Ask(name, TestRole);
                        continue;
                    }

                    bool resolved = false;
                    var agreedWith = new List<string>();
                    foreach (string control in Ordered(controls, firstControl, name))
                    {
                        // Loading a save onto itself says nothing the pair was meant to
                        // say, and the row would read as a control agreeing with its own
                        // test whatever happened.
                        if (string.Equals(control, name, StringComparison.Ordinal))
                        {
                            continue;
                        }

                        List<Answer> before = Ask(control, ControlRole);
                        List<Answer> after = Ask(name, TestRole);

                        // ANY expression differing is enough: one divergence proves the
                        // state changed, which is all the control was ever asked to show,
                        // and it shows it for the whole row.
                        if (Diverged(before, after))
                        {
                            resolved = true;
                            break;
                        }

                        agreedWith.Add(control);
                    }

                    report.Check(
                        resolved,
                        $"{name}: the load demonstrably took",
                        resolved
                            ? "its answer differs from the control before it"
                            : "it answered exactly as every control before it did, so "
                                + $"nothing here loaded it: {string.Join(", ", agreedWith)}");
                }

                if (!keepOpen)
                {
                    Console.WriteLine();
                    GameClosing.Quit(saveGames, process);
                }
            }
            catch (ProbePendingException pending)
            {
                // ProbeCommand can only say the game MAY not be running; this is the one
                // place that launched it and can say whether it is, and how it ended.
                throw new ProbePendingException(
                    $"{pending.Message} As for the game this run launched, "
                    + $"{GameExit.Describe(process)}.",
                    pending);
            }
            finally
            {
                // WRITTEN WHATEVER HAPPENED. A run that answered twenty saves and then lost
                // the game still learned twenty rows, and throwing them away would mean
                // starting over for nothing.
                Write(Path.Combine(artifacts, AnswersFileName), expressions, controls, answers);

                if (keepOpen)
                {
                    staged.Abandon();
                    Console.Error.WriteLine();
                    Console.Error.WriteLine(
                        "Left the game running, so NOTHING was restored. The player's profile "
                        + $"is at {staged.ProfileMovedTo} and their PlayerPrefs at "
                        + $"{staged.RegistryBackupPath}.");
                }
                else
                {
                    GameClosing.Close(process);
                    staged.Restore();
                }
            }

            Console.WriteLine();
            Console.WriteLine($"{report.Passed} of {report.Total} checks passed");
            Console.WriteLine($"answers in {Path.Combine(artifacts, AnswersFileName)}");
            if (report.Failures.Count > 0)
            {
                Console.WriteLine($"FAILED: {string.Join(", ", report.Failures)}");
            }

            return report.Failures.Count == 0 ? 0 : 1;
        }

        /// <summary>
        /// The controls to try for one save, with the predicted-best one first.
        /// </summary>
        /// <remarks>
        /// The ONLY thing a prediction does. Getting it right settles the row on the first
        /// control instead of the second; getting it wrong costs that second control and
        /// changes no answer, because what settles a row is a divergence this run saw.
        /// </remarks>
        private static IEnumerable<string> Ordered(
            IReadOnlyList<string> controls,
            IReadOnlyDictionary<string, string>? firstControl,
            string save)
        {
            if (firstControl == null
                || !firstControl.TryGetValue(save, out string? wanted))
            {
                return controls;
            }

            return new[] { wanted }
                .Concat(controls.Where(
                    control => !string.Equals(control, wanted, StringComparison.Ordinal)));
        }

        /// <summary>
        /// Whether a test save answered anything differently from the control before it.
        /// </summary>
        /// <remarks>
        /// Compared per expression, by what came back rather than by what anything was
        /// expected to say. An unread answer compares as itself, so a pair the game refused
        /// to answer twice does not read as a divergence.
        /// </remarks>
        private static bool Diverged(
            IReadOnlyList<Answer> before, IReadOnlyList<Answer> after)
        {
            foreach (Answer answer in after)
            {
                Answer? control = before.FirstOrDefault(
                    earlier => earlier.Expression == answer.Expression);
                if (control != null && control.Key() != answer.Key())
                {
                    return true;
                }
            }

            return false;
        }

        /// <summary>One expression's answer under one save.</summary>
        /// <remarks>
        /// READ IS SEPARATE FROM THE VALUE. An expression the game could not evaluate must
        /// not read as false - false is what most guards answer, so silence recorded as
        /// false would fill a truth table with plausible rows.
        /// </remarks>
        private sealed class Answer
        {
            public Answer(
                string save,
                string previous,
                string role,
                string expression,
                ProbeEvent answered)
            {
                Save = save;
                Previous = previous;
                Role = role;
                Expression = expression;
                Read = answered.Boolean("read") ?? false;
                Boolean = answered.Boolean("value");
                Number = answered.Number("value");
                Text = answered.Text("value");
            }

            /// <summary>The save this was asked under.</summary>
            public string Save { get; }

            /// <summary>
            /// The save loaded before it, which is what makes the row checkable.
            /// </summary>
            /// <remarks>
            /// Where a save's state does not take, the answer is about whatever was in
            /// before it - so an answer that differs from this one's is the proof that the
            /// load happened at all.
            /// </remarks>
            public string Previous { get; }

            /// <summary>Whether the row is one the table is about, or one setting it up.</summary>
            public string Role { get; }

            /// <summary>What was asked.</summary>
            public string Expression { get; }

            /// <summary>Whether the game answered at all.</summary>
            public bool Read { get; }

            /// <summary>The answer, where it was a boolean.</summary>
            public bool? Boolean { get; }

            /// <summary>The answer, where it was a number.</summary>
            public int? Number { get; }

            /// <summary>The answer, where it was a string.</summary>
            public string? Text { get; }

            /// <summary>
            /// The answer as one comparable token, kind and all.
            /// </summary>
            /// <remarks>
            /// The kind is part of it so that a string "true" and a boolean true are not
            /// mistaken for the same answer, which would hide a divergence rather than
            /// report one.
            /// </remarks>
            public string Key()
            {
                if (!Read)
                {
                    return "unread";
                }

                if (Boolean != null)
                {
                    return Boolean.Value ? "bool:true" : "bool:false";
                }

                if (Number != null)
                {
                    return "number:" + Number.Value.ToString(
                        System.Globalization.CultureInfo.InvariantCulture);
                }

                return Text == null ? "empty" : "text:" + Text;
            }

            /// <summary>The answer in one phrase, for the run's own output.</summary>
            public string Describe()
            {
                if (!Read)
                {
                    return "the game did not answer, which is not the same as false";
                }

                if (Boolean != null)
                {
                    return Boolean.Value ? "true" : "false";
                }

                if (Number != null)
                {
                    return Number.Value.ToString(
                        System.Globalization.CultureInfo.InvariantCulture);
                }

                return Text ?? "(answered, with nothing in it)";
            }
        }

        /// <summary>Writes the answers, as one row per save and expression.</summary>
        private static void Write(
            string path,
            IReadOnlyList<string> expressions,
            IReadOnlyList<string> controls,
            IReadOnlyList<Answer> answers)
        {
            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(
                buffer, new JsonWriterOptions { Indented = true }))
            {
                writer.WriteStartObject();

                writer.WriteStartArray("asked");
                foreach (string expression in expressions)
                {
                    writer.WriteStringValue(expression);
                }

                writer.WriteEndArray();

                writer.WriteStartArray("controls");
                foreach (string control in controls)
                {
                    writer.WriteStringValue(control);
                }

                writer.WriteEndArray();

                writer.WriteStartArray("answers");
                foreach (Answer answer in answers)
                {
                    writer.WriteStartObject();
                    writer.WriteString("save", answer.Save);
                    writer.WriteString("previous", answer.Previous);
                    writer.WriteString("role", answer.Role);
                    writer.WriteString("expression", answer.Expression);
                    writer.WriteBoolean("read", answer.Read);
                    if (answer.Boolean != null)
                    {
                        writer.WriteBoolean("value", answer.Boolean.Value);
                    }
                    else if (answer.Number != null)
                    {
                        writer.WriteNumber("value", answer.Number.Value);
                    }
                    else if (answer.Text != null)
                    {
                        writer.WriteString("value", answer.Text);
                    }

                    writer.WriteEndObject();
                }

                writer.WriteEndArray();
                writer.WriteEndObject();
            }

            File.WriteAllText(path, Encoding.UTF8.GetString(buffer.ToArray()) + "\n");
        }

        private static void Log(string message)
        {
            Console.WriteLine($"        {message}");
        }
    }
}
