// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Every probe event a log holds, read from where the last read stopped.
    /// </summary>
    /// <remarks>
    /// <para>THE LOG GROWS ALL RUN AND IS READ A FEW TIMES A SECOND, so re-parsing the
    /// whole of it per read makes noticing an event cost more the later in the run it
    /// arrives - on the machine the game is running on. This keeps the events it has
    /// already lifted out and asks the file only for what is new.</para>
    ///
    /// <para>A read stops at the last <see cref="ProbeLog.End"/> marker and leaves
    /// everything after it for the next one, so a chunk always begins and ends on a block
    /// boundary and the parser stays what it is - no partial line to carry across, no
    /// state to keep between chunks. The event being waited for is still seen the moment
    /// its closing marker lands, since the marker is the last thing its block writes.</para>
    ///
    /// <para>A file that SHRANK was replaced: BepInEx truncates its log when the game
    /// starts, and a run that launches the game twice would otherwise read the new log
    /// from an offset belonging to the old one. Everything is dropped and read again.</para>
    /// </remarks>
    public sealed class ProbeLogTail
    {
        private readonly string _path;
        private readonly List<ProbeEvent> _events = new List<ProbeEvent>();
        private ProbeEvent[] _all = Array.Empty<ProbeEvent>();
        private long _bytesRead;
        private bool _changed;

        /// <summary>Reads a BepInEx log.</summary>
        /// <param name="path">The log. It need not exist yet.</param>
        /// <exception cref="ArgumentNullException"><paramref name="path"/> is null.</exception>
        public ProbeLogTail(string path)
        {
            _path = path ?? throw new ArgumentNullException(nameof(path));
        }

        /// <summary>
        /// Every event the log holds, in order, including those read before.
        /// </summary>
        /// <remarks>
        /// A missing file is empty rather than an error: a run deletes the log before
        /// launching - which is what stops it reading the PREVIOUS run's events - so
        /// there is a window, until BepInEx creates its own, where the right answer is
        /// "nothing has happened yet".
        /// </remarks>
        public ProbeEvent[] Read()
        {
            if (File.Exists(_path))
            {
                TakeWhatIsNew();
            }

            if (_changed)
            {
                _all = _events.ToArray();
                _changed = false;
            }

            return _all;
        }

        private void TakeWhatIsNew()
        {
            // Shared: BepInEx holds the log open for writing for the whole run.
            using var stream = new FileStream(
                _path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);

            if (stream.Length < _bytesRead)
            {
                _bytesRead = 0;
                _events.Clear();
                _changed = true;
            }

            if (stream.Length == _bytesRead)
            {
                return;
            }

            stream.Seek(_bytesRead, SeekOrigin.Begin);
            string text = ReadRest(stream);

            // Through the last closing marker and no further. What follows it is either
            // nothing or a block the game is still writing.
            int lastEnd = text.LastIndexOf(ProbeLog.End, StringComparison.Ordinal);
            if (lastEnd < 0)
            {
                return;
            }

            string whole = text.Substring(0, lastEnd + ProbeLog.End.Length);
            _events.AddRange(ProbeLog.Read(whole));
            _bytesRead += Encoding.UTF8.GetByteCount(whole);
            _changed = true;
        }

        /// <summary>Everything from the stream's position to its end, as UTF-8 text.</summary>
        /// <remarks>
        /// Decoded in one go rather than through a StreamReader, because the position this
        /// stops at has to be expressible as a BYTE count for the next read to resume from
        /// - and a reader that buffers ahead has no answer to where it stopped.
        /// </remarks>
        private static string ReadRest(FileStream stream)
        {
            var bytes = new byte[stream.Length - stream.Position];
            int have = 0;
            while (have < bytes.Length)
            {
                int got = stream.Read(bytes, have, bytes.Length - have);
                if (got <= 0)
                {
                    break;
                }

                have += got;
            }

            return Encoding.UTF8.GetString(bytes, 0, have);
        }
    }
}
