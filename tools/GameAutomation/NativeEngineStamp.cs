// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Which source the deployed look-ahead library was built from, and whether it is
    /// still the source in front of you.
    /// </summary>
    /// <remarks>
    /// <para>WHAT THIS PREVENTS. An in-game run can test a library from days ago and report
    /// every check green. It did, 2026-09-04: the plugin's build prefers
    /// <c>target/release</c> where one exists, a session had built only <c>target/debug</c>,
    /// and the run passed 66 of 66 against an engine that did not contain the change under
    /// test. Nothing said so - not the deploy, which lists the files it copied; not the
    /// plugin's own commit stamp, which is the C# assembly's and the C# had been rebuilt;
    /// not the run log's revision, which is the working tree's rather than the binary's.
    /// It took an offline test disagreeing with a run that had just passed.</para>
    ///
    /// <para>THE COMPARISON IS A HASH OF THE LIBRARY'S SOURCES, not a commit. A commit cannot distinguish a
    /// library built before an uncommitted change from one built after it, and that is the
    /// case that actually happens while working. See <see cref="TreeHash"/>.</para>
    ///
    /// <para>NOT KNOWING IS NOT THE SAME AS DISAGREEING. A missing stamp, or one written
    /// where git could not answer, reports as unknown and says why; only two hashes that
    /// are both known and different are a staleness. A caller decides what to do with each
    /// - the in-game suites refuse, because their whole value is saying what the current
    /// code does.</para>
    /// </remarks>
    public static class NativeEngineStamp
    {
        /// <summary>What the stamp is called, beside the library it describes.</summary>
        public const string FileName = "GlobalConversationTracker.Native.built.json";

        /// <summary>What a stamp says when git could not answer.</summary>
        public const string NoRevision = "nogit";

        /// <summary>How a run's sources compare with the library it is about to run.</summary>
        public enum Freshness
        {
            /// <summary>The library was built from these sources.</summary>
            Current,

            /// <summary>It was built from different ones. The run would test those.</summary>
            Stale,

            /// <summary>Nothing can be said - no stamp, or no git to ask.</summary>
            Unknown,
        }

        /// <summary>What a check found, and how to say it.</summary>
        public sealed class Result
        {
            /// <summary>Creates a result.</summary>
            /// <param name="freshness">What was found.</param>
            /// <param name="what">One line saying it, for the report.</param>
            public Result(Freshness freshness, string what)
            {
                Freshness = freshness;
                What = what ?? throw new ArgumentNullException(nameof(what));
            }

            /// <summary>What was found.</summary>
            public Freshness Freshness { get; }

            /// <summary>One line saying it.</summary>
            public string What { get; }

            /// <inheritdoc/>
            public override string ToString() => What;
        }

        /// <summary>
        /// Whether the library deployed beside a game was built from this working tree.
        /// </summary>
        /// <param name="pluginFolder">The mod's folder inside the game's BepInEx plugins.</param>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>What was found, and how to say it.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="pluginFolder"/> is null.</exception>
        public static Result Check(string pluginFolder, string? repoRoot = null)
        {
            if (pluginFolder == null)
            {
                throw new ArgumentNullException(nameof(pluginFolder));
            }

            string path = Path.Combine(pluginFolder, FileName);
            if (!File.Exists(path))
            {
                return new Result(
                    Freshness.Unknown,
                    $"the look-ahead library carries no build stamp ({FileName} is not "
                    + "beside it), so what it was built from cannot be checked - deploy "
                    + "again to write one");
            }

            string? built;
            try
            {
                using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
                built = document.RootElement.TryGetProperty("tree", out JsonElement tree)
                    ? tree.GetString()
                    : null;
            }
            catch (Exception error)
            {
                return new Result(
                    Freshness.Unknown, $"{path} will not read: {error.Message}");
            }

            if (string.IsNullOrEmpty(built) || built == NoRevision)
            {
                return new Result(
                    Freshness.Unknown,
                    "the look-ahead library was built where git could not say what the "
                    + "sources were, so it cannot be checked against them");
            }

            string? now = TreeHash(repoRoot ?? GameInstall.RepoRoot());
            if (now == null)
            {
                return new Result(
                    Freshness.Unknown,
                    "git will not say what this working tree holds, so the deployed "
                    + "library cannot be checked against it");
            }

            if (string.Equals(now, built, StringComparison.Ordinal))
            {
                return new Result(
                    Freshness.Current,
                    $"the look-ahead library was built from this tree ({Short(now)})");
            }

            return new Result(
                Freshness.Stale,
                $"THE DEPLOYED LOOK-AHEAD LIBRARY IS NOT THIS CODE. It was built from "
                + $"{Short(built)} and the tree is at {Short(now)}, so a run would test "
                + "that older engine and could pass every check while saying nothing about "
                + "the change in front of you. Build it and deploy again:\n"
                + "    cargo build --release\n"
                + "    .\\deploy.ps1");
        }

        /// <summary>
        /// Everything the look-ahead library is built from, as git spells the paths.
        /// </summary>
        /// <remarks>
        /// RESTATED FROM <c>build.rs</c>, which writes the stamp this is compared against.
        /// The two have to ask git the same question and neither can call the other - one
        /// runs during a cargo build with no .NET in sight, the other in a harness that
        /// must work without cargo. A path added to one and not the other makes the check
        /// silently narrower, which is why each names the other.
        /// </remarks>
        public static readonly string[] Sources =
            { "Cargo.toml", "Cargo.lock", "build.rs", "src" };

        /// <summary>
        /// A hash over the library's sources, uncommitted changes to tracked files included.
        /// </summary>
        /// <remarks>
        /// <para><c>git add -u</c> into a COPY of the index, then <c>git ls-files -s</c>
        /// over <see cref="Sources"/>, hashed. The copy is what lets uncommitted changes
        /// count; the real index and the working tree are untouched, and <c>git status</c>
        /// is unchanged afterwards. Each line of the listing is a blob sha and a path, so
        /// hashing it hashes the CONTENT of every source without reading one.</para>
        ///
        /// <para>A COMMIT WOULD NOT DO. The case this exists for is a change that has been
        /// built but not committed, which is most of the time while working, and every
        /// commit-based stamp reads identical across it.</para>
        ///
        /// <para>ONLY THE LIBRARY'S SOURCES, which is the point rather than an
        /// optimisation. A hash over the whole tree changes when a harness file is edited,
        /// which happens constantly and has no bearing on what the library does - so the
        /// check would refuse sound runs, and a guard that cries wolf is one people learn
        /// to pass over.</para>
        /// </remarks>
        /// <param name="repoRoot">The repository root.</param>
        /// <returns>The hash, or null where git could not say.</returns>
        public static string? TreeHash(string repoRoot)
        {
            string index = Path.Combine(Path.GetTempPath(), "gct-tree-hash-index");
            try
            {
                File.Copy(Path.Combine(repoRoot, ".git", "index"), index, overwrite: true);
                if (Git(repoRoot, "add -u", index) == null)
                {
                    return null;
                }

                string? listed = Git(
                    repoRoot, "ls-files -s -- " + string.Join(" ", Sources), index);
                return listed == null ? null : Digest(listed);
            }
            catch (Exception)
            {
                // Only ever a label on a comparison; an unreadable index means unknown.
                return null;
            }
            finally
            {
                try
                {
                    File.Delete(index);
                }
                catch (Exception)
                {
                    // A temporary file that outlives the run costs nothing.
                }
            }
        }

        /// <summary>The first eight characters, which is enough to tell two apart.</summary>
        private static string Short(string hash) =>
            hash.Length <= 8 ? hash : hash.Substring(0, 8);

        /// <summary>
        /// A hex digest of a git listing, in the form <c>build.rs</c> writes.
        /// </summary>
        /// <remarks>
        /// FNV-1a over the raw bytes, 64 bits of it, restated from the build side for the
        /// same reason the path list is. Not a cryptographic hash and not trying to be: it
        /// compares one build's sources against one checkout's, where the only adversary is
        /// forgetfulness. The bytes must be exactly what the other side hashed, so the
        /// output is NOT trimmed - git's own line endings are part of what is hashed.
        /// </remarks>
        private static string Digest(string listing)
        {
            ulong hash = 0xcbf29ce484222325UL;
            foreach (byte value in System.Text.Encoding.UTF8.GetBytes(listing))
            {
                hash ^= value;
                hash *= 0x00000100000001b3UL;
            }

            return hash.ToString("x16", System.Globalization.CultureInfo.InvariantCulture);
        }

        /// <summary>One git command against a given index, or null if it failed.</summary>
        private static string? Git(string repoRoot, string arguments, string index)
        {
            var start = new ProcessStartInfo("git", arguments)
            {
                WorkingDirectory = repoRoot,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            start.Environment["GIT_INDEX_FILE"] = index;

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
                return git.ExitCode == 0 ? output : null;
            }
            catch (Exception)
            {
                return null;
            }
        }
    }
}
