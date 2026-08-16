using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnifiedConversationTracker.Persistence;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// A scratch directory standing in for the game's SaveGames directory.
    /// </summary>
    internal sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "uct-session-tests",
                Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        /// <summary>The directory, with no trailing separator.</summary>
        public string Path { get; }

        public UnifiedStateStore CreateStore() => new UnifiedStateStore(Path);

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, recursive: true);
            }
            catch (DirectoryNotFoundException)
            {
                // Nothing to clean up.
            }
        }
    }

    /// <summary>
    /// Captures every line the session logs, so "log loudly" can actually be
    /// asserted rather than hoped for.
    /// </summary>
    /// <remarks>
    /// Synchronized, because the background writer reports its own failures from its
    /// own thread (de-omm.22), so a real log genuinely does get written to
    /// concurrently. An unsynchronized <see cref="List{T}"/> would corrupt or throw
    /// under exactly the concurrency the writer tests exist to exercise. Reads are
    /// synchronized too, so an assertion made while a writer is still running sees a
    /// consistent snapshot rather than a list mid-resize.
    /// </remarks>
    internal sealed class RecordingLog : IUnifiedStateLog
    {
        private readonly object _gate = new object();
        private readonly List<string> _info = new List<string>();
        private readonly List<string> _warnings = new List<string>();
        private readonly List<string> _errors = new List<string>();
        private readonly List<string> _all = new List<string>();

        public List<string> Info => Snapshot(_info);

        public List<string> Warnings => Snapshot(_warnings);

        public List<string> Errors => Snapshot(_errors);

        /// <summary>Everything, in the order it was logged, prefixed by level.</summary>
        public List<string> All => Snapshot(_all);

        void IUnifiedStateLog.Info(string message) => Add(_info, "INFO ", message);

        void IUnifiedStateLog.Warning(string message) => Add(_warnings, "WARN ", message);

        void IUnifiedStateLog.Error(string message) => Add(_errors, "ERROR ", message);

        private void Add(List<string> level, string prefix, string message)
        {
            lock (_gate)
            {
                level.Add(message);
                _all.Add(prefix + message);
            }
        }

        private List<string> Snapshot(List<string> lines)
        {
            lock (_gate)
            {
                return new List<string>(lines);
            }
        }

        public bool AnyContains(IEnumerable<string> lines, string fragment) =>
            lines.Any(line => line.Contains(fragment, StringComparison.OrdinalIgnoreCase));

        public bool WarningOrErrorContains(string fragment) =>
            AnyContains(Warnings, fragment) || AnyContains(Errors, fragment);
    }

    /// <summary>
    /// A stand-in for the running game's SimStatus tables.
    /// </summary>
    internal sealed class FakeSimStatusSource : ISimStatusSource
    {
        private readonly List<SimStatusRow> _rows = new List<SimStatusRow>();

        public string Description { get; set; } = "fake game";

        public bool IsReady { get; set; } = true;

        /// <summary>Set to throw from the walk, standing in for a game-side failure.</summary>
        public Exception? ThrowOnEnumerate { get; set; }

        /// <summary>How many times the rows were actually walked.</summary>
        public int EnumerationCount { get; private set; }

        /// <summary>How many times readiness was polled.</summary>
        public int ReadinessCheckCount { get; private set; }

        public FakeSimStatusSource Add(int conversationId, int dialogueEntryId, string? statusName)
        {
            _rows.Add(new SimStatusRow(conversationId, dialogueEntryId, statusName));
            return this;
        }

        bool ISimStatusSource.IsReady
        {
            get
            {
                ReadinessCheckCount++;
                return IsReady;
            }
        }

        public IEnumerable<SimStatusRow> EnumerateSimStatuses()
        {
            EnumerationCount++;
            if (ThrowOnEnumerate != null)
            {
                throw ThrowOnEnumerate;
            }

            return _rows;
        }
    }
}
