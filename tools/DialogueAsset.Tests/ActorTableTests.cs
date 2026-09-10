// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>Reading the actor table out of a database, and writing it back.</summary>
    public class ActorTableTests
    {
        /// <summary>Two actors and the section after them, as Unity writes them.</summary>
        private const string Asset = """
              actors:
              - id: 1
                fields:
                - title: Name
                  value: Fysique
                  type: 0
                  typeString:
                - title: IsPlayer
                  value: False
                  type: 2
                  typeString: CustomFieldType_Boolean
                - title: Articy Id
                  value: 0x010000520000000D
                  type: 0
                  typeString:
                portrait: {fileID: 0}
                alternatePortraits: []
              - id: 424
                fields:
                - title: Name
                  value: Perception (Sight)
                  type: 0
                  typeString:
                portrait: {fileID: 0}
              items:
              - id: 1
                fields:
                - title: Name
                  value: Not an actor
                  type: 0
                  typeString:
            """;

        private static IReadOnlyList<DialogueActor> Read(string asset)
        {
            return ActorTableExtractor.Extract(new StringReader(asset));
        }

        [Fact]
        public void ReadsEveryActorsIdAndName()
        {
            IReadOnlyList<DialogueActor> actors = Read(Asset);

            Assert.Equal(2, actors.Count);
            Assert.Equal(1, actors[0].Id);
            Assert.Equal("Fysique", actors[0].Name);
            Assert.Equal(424, actors[1].Id);
            Assert.Equal("Perception (Sight)", actors[1].Name);
        }

        [Fact]
        public void StopsAtTheSectionAfterIt()
        {
            // The section that follows carries a Name field of its own, so a reader that
            // ran past the end would report it as an actor and shift every later id.
            Assert.DoesNotContain(Read(Asset), actor => actor.Name == "Not an actor");
        }

        [Fact]
        public void WritesOneObjectPerLine()
        {
            var written = new StringWriter { NewLine = "\n" };
            ActorTableFile.Write(written, Read(Asset));

            Assert.Equal(
                "{\"id\":1,\"name\":\"Fysique\"}\n"
                    + "{\"id\":424,\"name\":\"Perception (Sight)\"}\n",
                written.ToString());
        }
    }
}
