using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using UnifiedConversationTracker.CrashHarness;
using Xunit;

namespace UnifiedConversationTracker.Persistence.Tests
{
    /// <summary>
    /// The crash points, tested by actually killing a process part way through a
    /// save rather than by asserting that a simulated failure was handled.
    /// </summary>
    /// <remarks>
    /// Each test launches <see cref="UnifiedConversationTracker.CrashHarness"/> as a
    /// child process, which performs a real <see cref="UnifiedStateStore.Save"/> and
    /// calls <c>Process.Kill</c> on itself at the requested step: no stack unwinding,
    /// no finally blocks, no managed flush on the way out. The test then inspects the
    /// directory the dead process left behind. The in-process equivalents in
    /// <see cref="UnifiedStateStoreTests"/> are faster but weaker, because an
    /// exception still unwinds.
    /// </remarks>
    public class CrashPointTests
    {
        private const int HarnessTimeoutMilliseconds = 60_000;

        private sealed class HarnessRun
        {
            public HarnessRun(int exitCode, string standardOutput, string standardError)
            {
                ExitCode = exitCode;
                StandardOutput = standardOutput;
                StandardError = standardError;
            }

            public int ExitCode { get; }

            public string StandardOutput { get; }

            public string StandardError { get; }

            public override string ToString() =>
                $"exit {ExitCode}; stdout: {StandardOutput}; stderr: {StandardError}";
        }

        private static readonly Lazy<string> HarnessAssemblyPath = new Lazy<string>(FindHarnessAssembly);

        private static string FindHarnessAssembly()
        {
            const string harnessProjectName = "UnifiedConversationTracker.CrashHarness";
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory != null && !Directory.Exists(Path.Combine(directory.FullName, harnessProjectName)))
            {
                directory = directory.Parent;
            }

            if (directory == null)
            {
                throw new InvalidOperationException(
                    $"Could not find the '{harnessProjectName}' project directory above {AppContext.BaseDirectory}.");
            }

            // Two output layouts have to work. Without Directory.Build.props the harness
            // lands in <project>\bin\<config>\<tfm>\; with it, output is redirected to
            // .build\bin\<project>\<config>\<tfm>\ and there is no "bin" segment below the
            // project folder at all. Searching the project folder itself covers both.
            string searchRoot = Path.Combine(directory.FullName, harnessProjectName);
            string[] candidates = Directory.Exists(searchRoot)
                ? Directory.GetFiles(searchRoot, harnessProjectName + ".dll", SearchOption.AllDirectories)
                    // obj holds ref/refint reference assemblies, which carry no method
                    // bodies and cannot be executed. Never hand one to the runner.
                    .Where(path => !path.Split(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
                        .Contains("obj", StringComparer.OrdinalIgnoreCase))
                    .ToArray()
                : Array.Empty<string>();

            if (candidates.Length == 0)
            {
                throw new InvalidOperationException(
                    $"The crash harness has not been built; no runnable {harnessProjectName}.dll under '{searchRoot}'. " +
                    $"Build it first: dotnet build src\\{harnessProjectName}");
            }

            // Most recently built configuration wins, so the tests exercise whatever was
            // just compiled alongside them.
            return candidates.OrderByDescending(File.GetLastWriteTimeUtc).First();
        }

        private static HarnessRun RunHarness(string directory, string payload, string step)
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = Environment.GetEnvironmentVariable("DOTNET_HOST_PATH") ?? "dotnet",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
            };
            startInfo.ArgumentList.Add("exec");
            startInfo.ArgumentList.Add(HarnessAssemblyPath.Value);
            startInfo.ArgumentList.Add(directory);
            startInfo.ArgumentList.Add(payload);
            startInfo.ArgumentList.Add(step);

            using Process process = Process.Start(startInfo)
                ?? throw new InvalidOperationException("Could not start the crash harness.");

            string standardOutput = process.StandardOutput.ReadToEnd();
            string standardError = process.StandardError.ReadToEnd();
            if (!process.WaitForExit(HarnessTimeoutMilliseconds))
            {
                process.Kill(entireProcessTree: true);
                throw new TimeoutException($"The crash harness did not exit within {HarnessTimeoutMilliseconds} ms.");
            }

            return new HarnessRun(process.ExitCode, standardOutput, standardError);
        }

        private static void SaveOldGenerationCleanly(TempDirectory temp)
        {
            HarnessRun run = RunHarness(temp.Path, CrashHarnessPayloads.OldName, Program.NoCrashStep);
            Assert.True(run.ExitCode == 0, $"Setup save failed: {run}");
        }

        private static HarnessRun CrashSavingNewGeneration(TempDirectory temp, UnifiedStateSaveStep step)
        {
            HarnessRun run = RunHarness(temp.Path, CrashHarnessPayloads.NewName, step.ToString());

            // A zero exit code would mean the process ran to completion and the "crash"
            // never happened, which would make every assertion below meaningless.
            Assert.True(run.ExitCode != 0, $"The harness was supposed to be killed at {step} but exited cleanly: {run}");
            Assert.NotEqual(Program.ExitStepNotReached, run.ExitCode);
            Assert.NotEqual(Program.ExitBadUsage, run.ExitCode);
            return run;
        }

        private static void AssertHoldsPayload(UnifiedStateLoadResult result, string payloadName)
        {
            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(
                CrashHarnessPayloads.Build(payloadName).EnumerateEntriesInIdOrder().ToList(),
                result.RequireState().EnumerateEntriesInIdOrder().ToList());
        }

        /// <summary>
        /// The invariant the whole design exists to protect: after a crash at any
        /// step, some complete file is still on disk and still loads.
        /// </summary>
        private static void AssertACompleteFileSurvives(UnifiedStateStore store, string expectedPayloadName)
        {
            UnifiedStateRecovery recovery = store.LoadWithBackupFallback();
            IEnumerable<string?> files = Directory.GetFiles(store.DirectoryPath).Select(Path.GetFileName);
            Assert.True(
                recovery.IsLoaded,
                $"No complete file survived: {recovery}. Directory contents: {string.Join(", ", files)}");
            AssertHoldsPayload(recovery.Effective, expectedPayloadName);
        }

        [Fact]
        public void HarnessWithoutACrash_WritesTheNewGeneration()
        {
            // Control case: proves the harness really does save, so the crash cases are
            // measuring a killed save rather than a broken harness.
            using var temp = new TempDirectory();
            UnifiedStateStore store = temp.CreateStore();

            HarnessRun run = RunHarness(temp.Path, CrashHarnessPayloads.NewName, Program.NoCrashStep);

            Assert.True(run.ExitCode == 0, run.ToString());
            AssertHoldsPayload(store.Load(), CrashHarnessPayloads.NewName);
        }

        [Fact]
        public void KilledDuringTheTempWrite_LivesOnTheOldGeneration()
        {
            using var temp = new TempDirectory();
            UnifiedStateStore store = temp.CreateStore();
            SaveOldGenerationCleanly(temp);

            CrashSavingNewGeneration(temp, UnifiedStateSaveStep.DuringTempWrite);

            // A genuinely truncated temp file is on disk...
            Assert.True(File.Exists(store.TempPath));
            long completeLength = UnifiedStateJson
                .SerializeToUtf8Bytes(CrashHarnessPayloads.Build(CrashHarnessPayloads.NewName)).Length;
            long tempLength = new FileInfo(store.TempPath).Length;
            Assert.InRange(tempLength, 1, completeLength - 1);
            Assert.Equal(
                UnifiedStateLoadOutcome.Corrupt,
                UnifiedStateJson.Deserialize(File.ReadAllBytes(store.TempPath), store.TempPath).Outcome);

            // ...and it is irrelevant, because the live file was never touched.
            AssertHoldsPayload(store.Load(), CrashHarnessPayloads.OldName);
            AssertACompleteFileSurvives(store, CrashHarnessPayloads.OldName);
        }

        [Fact]
        public void KilledAfterTheTempFlush_LivesOnTheOldGeneration()
        {
            using var temp = new TempDirectory();
            UnifiedStateStore store = temp.CreateStore();
            SaveOldGenerationCleanly(temp);

            CrashSavingNewGeneration(temp, UnifiedStateSaveStep.AfterTempFlushed);

            // The temp file is complete here, but it is not the live file and is never
            // trusted on load.
            Assert.True(File.Exists(store.TempPath));
            AssertHoldsPayload(store.Load(), CrashHarnessPayloads.OldName);
            AssertACompleteFileSurvives(store, CrashHarnessPayloads.OldName);
        }

        [Fact]
        public void KilledAfterTheRotation_RecoversTheOldGenerationFromTheBackup()
        {
            // The only window in which no live file exists. Without the backup
            // generation this crash would lose everything.
            using var temp = new TempDirectory();
            UnifiedStateStore store = temp.CreateStore();
            SaveOldGenerationCleanly(temp);

            CrashSavingNewGeneration(temp, UnifiedStateSaveStep.AfterLiveRotatedToBackup);

            Assert.False(File.Exists(store.LivePath));
            Assert.Equal(UnifiedStateLoadOutcome.Missing, store.Load().Outcome);
            AssertHoldsPayload(store.LoadBackup(), CrashHarnessPayloads.OldName);
            AssertACompleteFileSurvives(store, CrashHarnessPayloads.OldName);
        }

        [Fact]
        public void KilledAtAnyStep_ThenSavingAgain_Succeeds()
        {
            // Whatever wreckage a crash left, the next save must still land and must
            // leave a loadable live file plus a loadable backup.
            foreach (UnifiedStateSaveStep step in Enum.GetValues<UnifiedStateSaveStep>())
            {
                if (step == UnifiedStateSaveStep.AfterTempPromoted)
                {
                    // Nothing follows this step, so killing there is just a completed save.
                    continue;
                }

                using var temp = new TempDirectory();
                UnifiedStateStore store = temp.CreateStore();
                SaveOldGenerationCleanly(temp);
                CrashSavingNewGeneration(temp, step);

                HarnessRun recoverySave = RunHarness(temp.Path, CrashHarnessPayloads.NewName, Program.NoCrashStep);

                Assert.True(recoverySave.ExitCode == 0, $"Save after a crash at {step} failed: {recoverySave}");
                AssertHoldsPayload(store.Load(), CrashHarnessPayloads.NewName);
                Assert.Equal(UnifiedStateLoadOutcome.Loaded, store.LoadBackup().Outcome);
                Assert.False(File.Exists(store.TempPath), $"A temp file survived the save after a crash at {step}.");
            }
        }

        [Fact]
        public void KilledAtTheFinalPromotion_HasAlreadyLandedTheNewGeneration()
        {
            // AfterTempPromoted fires once the rename is done, so even an immediate kill
            // there leaves the new state live and the old one in the backup slot.
            using var temp = new TempDirectory();
            UnifiedStateStore store = temp.CreateStore();
            SaveOldGenerationCleanly(temp);

            CrashSavingNewGeneration(temp, UnifiedStateSaveStep.AfterTempPromoted);

            AssertHoldsPayload(store.Load(), CrashHarnessPayloads.NewName);
            AssertHoldsPayload(store.LoadBackup(), CrashHarnessPayloads.OldName);
        }
    }
}
