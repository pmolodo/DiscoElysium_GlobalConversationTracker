// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// A run's console output, kept in <c>testing/logs</c> under a name that says when it
    /// ran, what ran, and what it ran against.
    /// </summary>
    /// <remarks>
    /// <para>WHY IT IS NOT LEFT TO WHOEVER LAUNCHES THE RUN. It was, and the logs that
    /// survived were the ones someone remembered to tee. The runs worth reading afterwards
    /// are the long ones - an in-game suite, a measurement - which are exactly the ones
    /// nobody wants to start again because the output scrolled away or a filter ate it.
    /// A run that logs itself cannot be launched wrong.</para>
    ///
    /// <para>THE NAME IS THE POINT. A log that cannot be tied to a commit is a wall of
    /// text; one that can is evidence, and two of them are a diff. Hence the revision in
    /// the name, and hence <see cref="DirtySuffix"/> - a sha that does not describe what
    /// actually ran is worse than no sha at all, because it invites a later reader to
    /// diff against a commit that never contained the code under test.</para>
    ///
    /// <para>The same format is produced by <c>tools/run-logged.sh</c>, which wraps the
    /// runs that are not ours to modify - cargo, dotnet test. A test holds the two to
    /// each other.</para>
    /// </remarks>
    public static class RunLog
    {
        /// <summary>What is appended to a revision taken from a dirty tree.</summary>
        public const string DirtySuffix = "-dirty";

        /// <summary>Stands in for the revision where git cannot say what it is.</summary>
        public const string NoRevision = "nogit";

        /// <summary>Set to opt out of logging entirely.</summary>
        public const string OptOutVariable = "DISCO_ELYSIUM_GCT_NO_RUN_LOG";

        /// <summary>The date part's format, and the first thing a name sorts by.</summary>
        public const string DateFormat = "yyyy-MM-dd";

        private const string Extension = ".txt";

        /// <summary>Where the logs live, relative to the repository root.</summary>
        private static readonly string[] FolderParts = { "testing", "logs" };

        /// <summary>The folder the logs are written to, created if it is not there.</summary>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>The folder's path.</returns>
        public static string Folder(string? repoRoot = null)
        {
            string folder = Path.Combine(
                repoRoot ?? GameInstall.RepoRoot(), FolderParts[0], FolderParts[1]);
            Directory.CreateDirectory(folder);
            return folder;
        }

        /// <summary>
        /// The name a run's log takes: the date, the revision, the tool and its verb.
        /// </summary>
        /// <param name="when">When the run started.</param>
        /// <param name="revision">What it ran against, from <see cref="Revision"/>.</param>
        /// <param name="tool">What ran - <c>GameHarness</c>, <c>cargo</c>.</param>
        /// <param name="verb">Which run it was - <c>look-ahead</c>, <c>corpus</c>.</param>
        /// <returns>A file name, extension included.</returns>
        /// <exception cref="ArgumentException">The tool or verb is empty.</exception>
        public static string FileName(
            DateTime when, string revision, string tool, string verb)
        {
            return string.Join(
                "_",
                when.ToString(DateFormat, CultureInfo.InvariantCulture),
                Safe(revision, nameof(revision)),
                Safe(tool, nameof(tool)),
                Safe(verb, nameof(verb))) + Extension;
        }

        /// <summary>
        /// What the repository is sitting at: the commit, plus <see cref="DirtySuffix"/>
        /// when the tree has been changed since.
        /// </summary>
        /// <remarks>
        /// Answers <see cref="NoRevision"/> rather than throwing when git cannot say -
        /// no git, no repository, a fresh clone with no commit yet. Losing the whole log
        /// of a ten-minute run over a missing label is the wrong trade; a name that
        /// admits it does not know is not.
        /// </remarks>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>The full sha, possibly suffixed, or <see cref="NoRevision"/>.</returns>
        public static string Revision(string? repoRoot = null)
        {
            string root = repoRoot ?? GameInstall.RepoRoot();
            string? head = Git(root, "rev-parse HEAD");
            if (string.IsNullOrEmpty(head))
            {
                return NoRevision;
            }

            // Ignored files - the logs themselves among them - are not reported by
            // --porcelain, so writing this log does not make the next one dirty.
            string? changes = Git(root, "status --porcelain");
            return changes == null || changes.Length == 0 ? head! : head + DirtySuffix;
        }

        /// <summary>
        /// The given path, or the first numbered variant of it that is free.
        /// </summary>
        /// <remarks>
        /// TWO RUNS AT ONE COMMIT ON ONE DAY IS THE NORMAL CASE, not the exception -
        /// it is what iterating on a failure looks like. Overwriting would throw away the
        /// run being compared against, which is the whole reason the logs are kept.
        /// </remarks>
        /// <param name="path">The name that was wanted.</param>
        /// <returns>A path nothing is at yet.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="path"/> is null.</exception>
        public static string Unique(string path)
        {
            if (path == null)
            {
                throw new ArgumentNullException(nameof(path));
            }

            if (!File.Exists(path))
            {
                return path;
            }

            string folder = Path.GetDirectoryName(path) ?? string.Empty;
            string stem = Path.GetFileNameWithoutExtension(path);
            string extension = Path.GetExtension(path);
            for (int attempt = 2; ; attempt++)
            {
                string candidate = Path.Combine(
                    folder,
                    stem + "_" + attempt.ToString(CultureInfo.InvariantCulture) + extension);
                if (!File.Exists(candidate))
                {
                    return candidate;
                }
            }
        }

        /// <summary>The path this run's log should be written to.</summary>
        /// <param name="tool">What is running.</param>
        /// <param name="verb">Which run it is.</param>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>A full path in <c>testing/logs</c> that nothing is at yet.</returns>
        public static string PathFor(string tool, string verb, string? repoRoot = null)
        {
            string root = repoRoot ?? GameInstall.RepoRoot();
            return Unique(Path.Combine(
                Folder(root),
                FileName(DateTime.Now, Revision(root), tool, verb)));
        }

        /// <summary>Whether logging has been turned off for this process.</summary>
        public static bool OptedOut
        {
            get
            {
                string? value = Environment.GetEnvironmentVariable(OptOutVariable);
                return !string.IsNullOrWhiteSpace(value)
                    && !string.Equals(value, "0", StringComparison.Ordinal)
                    && !string.Equals(value, "false", StringComparison.OrdinalIgnoreCase);
            }
        }

        /// <summary>
        /// Starts copying everything written to the console into a log file, until the
        /// returned handle is disposed.
        /// </summary>
        /// <param name="tool">What is running.</param>
        /// <param name="verb">Which run it is.</param>
        /// <param name="banner">The command as it was typed, for the file's first line.</param>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>The running log, or null when <see cref="OptedOut"/>.</returns>
        public static IDisposable? Begin(
            string tool, string verb, string? banner = null, string? repoRoot = null)
        {
            return OptedOut ? null : new Session(PathFor(tool, verb, repoRoot), banner);
        }

        /// <summary>One name component, with anything awkward taken out of it.</summary>
        private static string Safe(string part, string parameterName)
        {
            if (string.IsNullOrWhiteSpace(part))
            {
                throw new ArgumentException("A log name needs one.", parameterName);
            }

            var safe = new StringBuilder(part.Length);
            foreach (char character in part)
            {
                safe.Append(
                    char.IsLetterOrDigit(character)
                        || character == '-' || character == '.' ? character : '-');
            }

            return safe.ToString();
        }

        /// <summary>One git command, or null if it could not be run or failed.</summary>
        private static string? Git(string repoRoot, string arguments)
        {
            var start = new ProcessStartInfo("git", arguments)
            {
                WorkingDirectory = repoRoot,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };

            try
            {
                using Process? git = Process.Start(start);
                if (git == null)
                {
                    return null;
                }

                string output = git.StandardOutput.ReadToEnd();
                git.StandardError.ReadToEnd();
                git.WaitForExit();
                return git.ExitCode == 0 ? output.Trim() : null;
            }
            catch (Exception)
            {
                // Only ever a label on a file name; see the remarks on Revision.
                return null;
            }
        }

        /// <summary>The console, mirrored into a file for as long as this lives.</summary>
        private sealed class Session : IDisposable
        {
            private readonly TextWriter _console;
            private readonly TextWriter _error;
            private readonly StreamWriter _file;
            private readonly string _path;
            private bool _closed;

            internal Session(string path, string? banner)
            {
                _path = path;

                // AUTOFLUSH, ALWAYS. A run that is killed - or that takes the process
                // down, which the measurements do routinely - must leave behind what it
                // had said up to that point. A buffered log of a crash is an empty file,
                // and an empty file has already once been read as "the run died".
                _file = new StreamWriter(path, append: false) { AutoFlush = true };
                if (!string.IsNullOrEmpty(banner))
                {
                    _file.WriteLine("# " + banner);
                    _file.WriteLine();
                }

                // One file, two writers into it - out and error - so the handed-out
                // writer has to be the synchronized one or the two interleave mid-line.
                TextWriter shared = TextWriter.Synchronized(_file);
                _console = Console.Out;
                _error = Console.Error;
                Console.SetOut(new Tee(_console, shared));
                Console.SetError(new Tee(_error, shared));
                _console.WriteLine($"logging to {path}");
            }

            public void Dispose()
            {
                if (_closed)
                {
                    return;
                }

                _closed = true;
                Console.SetOut(_console);
                Console.SetError(_error);
                _file.Dispose();
                _console.WriteLine($"logged to {_path}");
            }
        }

        /// <summary>Writes everything twice.</summary>
        private sealed class Tee : TextWriter
        {
            private readonly TextWriter _first;
            private readonly TextWriter _second;

            internal Tee(TextWriter first, TextWriter second)
            {
                _first = first;
                _second = second;
            }

            public override Encoding Encoding => _first.Encoding;

            public override void Write(char value)
            {
                _first.Write(value);
                _second.Write(value);
            }

            public override void Write(string? value)
            {
                _first.Write(value);
                _second.Write(value);
            }

            public override void Flush()
            {
                _first.Flush();
                _second.Flush();
            }
        }
    }
}
