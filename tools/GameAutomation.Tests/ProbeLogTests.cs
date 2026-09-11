// SPDX-License-Identifier: MIT
using System;
using System.Linq;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Reading what the in-game probe wrote into the BepInEx log.</summary>
    public class ProbeLogTests
    {
        /// <summary>The colour the mod paints "leads somewhere no save has seen".</summary>
        private const string Orange = "#FF8C42";

        /// <summary>The colour for "leads somewhere this save has not seen".</summary>
        private const string Red = "#C4453C";

        private const string Prefix = "[Message:GlobalConversationTrackerTestProbe] ";

        /// <summary>Wraps a JSON body the way the probe writes it into the log.</summary>
        private static string Block(string json)
        {
            return string.Join("\n", new[]
            {
                Prefix + ProbeLog.Begin,
                Prefix + json,
                Prefix + ProbeLog.End,
            });
        }

        [Fact]
        public void AnEventIsLiftedOutFromBetweenTheMarkers()
        {
            ProbeEvent[] events = ProbeLog.Read(
                Block("{\"event\":\"world-ready\",\"money\":5100}"));

            Assert.Single(events);
            Assert.Equal("world-ready", events[0].Name);
            Assert.Equal(5100, events[0].Number("money"));
        }

        [Fact]
        public void EverythingOutsideTheMarkersIsIgnored()
        {
            string log = string.Join("\n", new[]
            {
                "[Info:BepInEx] Chainloader started",
                "[Message:GlobalConversationTracker] Hooked something; asterisks are on.",
                Block("{\"event\":\"ready\",\"version\":\"0.1.0\"}"),
                "[Warning:Something] a stray } and a { for good measure",
            });

            ProbeEvent[] events = ProbeLog.Read(log);

            Assert.Single(events);
            Assert.Equal("ready", events[0].Name);
        }

        [Fact]
        public void EventsComeBackInOrder()
        {
            string log = string.Join("\n", new[]
            {
                Block("{\"event\":\"ready\"}"),
                Block("{\"event\":\"world-ready\"}"),
                Block("{\"event\":\"save-applied\"}"),
                Block("{\"event\":\"menu\"}"),
            });

            Assert.Equal(
                new[] { "ready", "world-ready", "save-applied", "menu" },
                ProbeLog.Read(log).Select(e => e.Name).ToArray());
        }

        [Fact]
        public void AMenuCarriesEveryOptionWithItsMarkup()
        {
            string json =
                "{\"event\":\"menu\",\"conversation\":451,\"money\":5100,\"shown\":2,"
                + "\"options\":["
                + "{\"conversation\":451,\"entry\":86,"
                + "\"text\":\"You: \\\"I *need* those FALN sneakers.\\\""
                + "<color=" + Orange + ">*</color>\"},"
                + "{\"conversation\":451,\"entry\":33,\"text\":\"You: \\\"Never mind.\\\"\"}"
                + "]}";

            ProbeEvent[] events = ProbeLog.Read(Block(json));
            ProbeOption[] options = events[0].Options();

            Assert.Equal(451, events[0].Number("conversation"));
            Assert.Equal(2, options.Length);

            Assert.Equal(86, options[0].EntryId);
            Assert.Equal("You: \"I *need* those FALN sneakers.\"", options[0].Text!.Split('<')[0]);
            Assert.True(options[0].HasMarker(Orange));
            Assert.False(options[0].HasMarker(Red));

            Assert.Equal(33, options[1].EntryId);
            Assert.False(options[1].HasMarker(Orange));
            Assert.False(options[1].HasMarker(Red));
        }

        [Fact]
        public void AnEventWithNoOptionsHasNone()
        {
            ProbeEvent[] events = ProbeLog.Read(Block("{\"event\":\"world-ready\"}"));

            Assert.Empty(events[0].Options());
        }

        [Fact]
        public void AMissingMemberIsNullRatherThanAnError()
        {
            ProbeEvent[] events = ProbeLog.Read(Block("{\"event\":\"world-ready\"}"));

            Assert.Null(events[0].Text("money"));
            Assert.Null(events[0].Number("money"));
        }

        [Fact]
        public void ABlockCutOffMidWriteCostsOnlyItself()
        {
            // What a killed game leaves behind. The run's earlier events still matter.
            string log = string.Join("\n", new[]
            {
                Block("{\"event\":\"world-ready\",\"money\":5100}"),
                Prefix + ProbeLog.Begin,
                Prefix + "{\"event\":\"menu\",\"opti",
            });

            ProbeEvent[] events = ProbeLog.Read(log);

            Assert.Single(events);
            Assert.Equal("world-ready", events[0].Name);
        }

        [Fact]
        public void AMarkerBeginningBeforeTheLastOneEndedDiscardsThePartialBlock()
        {
            string log = string.Join("\n", new[]
            {
                Prefix + ProbeLog.Begin,
                Prefix + "{\"event\":\"menu\",\"opti",
                Prefix + ProbeLog.Begin,
                Prefix + "{\"event\":\"world-ready\"}",
                Prefix + ProbeLog.End,
            });

            ProbeEvent[] events = ProbeLog.Read(log);

            Assert.Single(events);
            Assert.Equal("world-ready", events[0].Name);
        }

        [Fact]
        public void CarriageReturnsDoNotReachTheParser()
        {
            string log = Block("{\"event\":\"menu\",\"conversation\":451}").Replace("\n", "\r\n");

            ProbeEvent[] events = ProbeLog.Read(log);

            Assert.Single(events);
            Assert.Equal(451, events[0].Number("conversation"));
        }

        [Fact]
        public void JsonContainingABracketIsNotMistakenForALogPrefix()
        {
            // The prefix is only stripped when what follows starts an object, so a
            // "] " inside an option's text survives.
            string json = "{\"event\":\"menu\",\"options\":[{\"entry\":1,"
                + "\"text\":\"[Kim] said: done. \"}]}";

            ProbeEvent[] events = ProbeLog.Read(Block(json));

            Assert.Equal("[Kim] said: done. ", events[0].Options()[0].Text);
        }

        [Fact]
        public void AnEscapedColourTagIsStillFoundAsAMarker()
        {
            // The probe asks for the relaxed encoder so the tag stays readable in the
            // log, but JSON's HTML-safe default escapes the angle brackets. A reader
            // must not care which one wrote the file.
            string json = "{\"event\":\"menu\",\"options\":[{\"entry\":86,"
                + "\"text\":\"buy them\\u003Ccolor=" + Orange + "\\u003E*\\u003C/color\\u003E\"}]}";

            ProbeOption option = ProbeLog.Read(Block(json))[0].Options()[0];

            Assert.Equal("buy them<color=" + Orange + ">*</color>", option.Text);
            Assert.True(option.HasMarker(Orange));
        }

        [Fact]
        public void AnAsteriskInTheWritingIsNotAMarker()
        {
            // "I *need* those FALN sneakers" is the text of an option these tests click.
            Assert.False(
                ProbeLog.HasMarker("You: \"I *need* those FALN sneakers.\"", Orange));
        }

        [Fact]
        public void NoTextCarriesNoMarker()
        {
            Assert.False(ProbeLog.HasMarker(null, Orange));
            Assert.False(ProbeLog.HasMarker(string.Empty, Orange));
        }

        [Fact]
        public void AColourIsRequiredToAskAboutAMarker()
        {
            Assert.Throws<ArgumentNullException>(() => ProbeLog.HasMarker("anything", null!));
        }

        [Fact]
        public void NothingIsRefusedRatherThanRead()
        {
            Assert.Throws<ArgumentNullException>(() => ProbeLog.Read(null!));
        }
    }
}
