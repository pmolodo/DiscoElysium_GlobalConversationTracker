// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Reading a growing probe log from where the last read stopped.</summary>
    public class ProbeLogTailTests : IDisposable
    {
        private const string Prefix = "[Message:GlobalConversationTrackerTestProbe] ";

        private readonly string _root;
        private readonly string _log;

        public ProbeLogTailTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-tail-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_root);
            _log = Path.Combine(_root, "LogOutput.log");
        }

        public void Dispose()
        {
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }

            GC.SuppressFinalize(this);
        }

        /// <summary>One event as the probe writes it, closing marker and all.</summary>
        private static string Block(string json)
        {
            return string.Join(
                "\n",
                new[] { Prefix + ProbeLog.Begin, Prefix + json, Prefix + ProbeLog.End })
                + "\n";
        }

        private static string Named(string name) => Block($"{{\"event\":\"{name}\"}}");

        private void Append(string text)
        {
            File.AppendAllText(_log, text, new UTF8Encoding(false));
        }

        [Fact]
        public void ALogThatIsNotThereYetHasNothingInIt()
        {
            Assert.Empty(new ProbeLogTail(_log).Read());
        }

        [Fact]
        public void EveryEventComesBackIncludingThoseReadBefore()
        {
            var tail = new ProbeLogTail(_log);
            Append(Named("ready"));
            Assert.Equal(new[] { "ready" }, Names(tail.Read()));

            Append(Named("world-ready"));

            Assert.Equal(new[] { "ready", "world-ready" }, Names(tail.Read()));
        }

        [Fact]
        public void AReadThatFindsNothingNewRepeatsItself()
        {
            var tail = new ProbeLogTail(_log);
            Append(Named("ready"));
            tail.Read();

            Assert.Equal(new[] { "ready" }, Names(tail.Read()));
        }

        /// <summary>
        /// The half-written block is what a read lands in the middle of, since the game
        /// writes one while the harness is waiting for it.
        /// </summary>
        [Fact]
        public void ABlockWithoutItsClosingMarkerWaitsForOne()
        {
            var tail = new ProbeLogTail(_log);
            Append(Named("ready"));
            Append(Prefix + ProbeLog.Begin + "\n" + Prefix + "{\"event\":\"menu\"}\n");

            Assert.Equal(new[] { "ready" }, Names(tail.Read()));

            Append(Prefix + ProbeLog.End + "\n");

            Assert.Equal(new[] { "ready", "menu" }, Names(tail.Read()));
        }

        /// <summary>
        /// An option's text is arbitrary prose, and this game's is not ASCII. A byte
        /// offset taken as a character count would resume mid-word and lose the next
        /// event.
        /// </summary>
        [Fact]
        public void TextOutsideASCIIDoesNotThrowOffWhereTheNextReadBegins()
        {
            var tail = new ProbeLogTail(_log);
            Append(Block("{\"event\":\"menu\",\"text\":\"Kras Mazov - a \\u00e9migr\\u00e9\"}"));
            Assert.Single(tail.Read());

            Append(Named("world-ready"));

            Assert.Equal(new[] { "menu", "world-ready" }, Names(tail.Read()));
        }

        /// <summary>
        /// BepInEx truncates its log when the game starts, so a run that launches twice
        /// reads a new file through an offset belonging to the old one.
        /// </summary>
        [Fact]
        public void ALogThatShrankIsReadFromTheBeginningAgain()
        {
            var tail = new ProbeLogTail(_log);
            Append(Named("ready"));
            Append(Named("world-ready"));
            Assert.Equal(2, tail.Read().Length);

            File.WriteAllText(_log, Named("ready"), new UTF8Encoding(false));

            Assert.Equal(new[] { "ready" }, Names(tail.Read()));
        }

        [Fact]
        public void EverythingOutsideTheMarkersIsIgnored()
        {
            var tail = new ProbeLogTail(_log);
            Append("[Info:BepInEx] Chainloader started\n");
            Append(Named("ready"));
            Append("[Warning:Something] a stray } and a { for good measure\n");

            Assert.Equal(new[] { "ready" }, Names(tail.Read()));
        }

        [Fact]
        public void NoPathIsRefusedRatherThanRead()
        {
            Assert.Throws<ArgumentNullException>(() => new ProbeLogTail(null!));
        }

        private static string[] Names(ProbeEvent[] events)
        {
            var names = new string[events.Length];
            for (int i = 0; i < events.Length; i++)
            {
                names[i] = events[i].Name;
            }

            return names;
        }
    }
}
