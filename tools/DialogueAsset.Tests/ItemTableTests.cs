// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>Reading the item table out of a database, and writing it back.</summary>
    /// <remarks>
    /// The item table had no tests at all until the group was added to it, which is worth
    /// saying because the suite was green throughout: the other six classes cover the actor
    /// table, the corpus, the index and the scalars, and none of them touches this reader.
    /// </remarks>
    public class ItemTableTests
    {
        /// <summary>
        /// Four records and the section after them, as Unity writes them.
        /// </summary>
        /// <remarks>
        /// The fourth is a THOUGHT, which shares the items section in the shipped database -
        /// 53 of its 259 records are thoughts, and they carry no itemGroup at all.
        /// </remarks>
        private const string Asset = """
              items:
              - id: 1
                fields:
                - title: Name
                  value: drug_alcohol_commodore_red
                  type: 0
                  typeString:
                - title: itemGroup
                  value: 1
                  type: 1
                  typeString: CustomFieldType_Number
                - title: stackName
                  value:
                  type: 0
                  typeString:
              - id: 2
                fields:
                - title: Name
                  value: key_trash_container
                  type: 0
                  typeString:
                - title: stackName
                  value: key_ring
                  type: 0
                  typeString:
                - title: itemGroup
                  value: 0
                  type: 1
                  typeString: CustomFieldType_Number
              - id: 3
                fields:
                - title: Name
                  value: glass_tare
                  type: 0
                  typeString:
                - title: itemGroup
                  value: 6
                  type: 1
                  typeString: CustomFieldType_Number
              - id: 4
                fields:
                - title: Name
                  value: revacholian_nationhood
                  type: 0
                  typeString:
                - title: isThought
                  value: True
                  type: 2
                  typeString: CustomFieldType_Boolean
              conversations:
              - id: 1
                fields:
                - title: Name
                  value: Not an item
                  type: 0
                  typeString:
            """;

        private static IReadOnlyList<DialogueItem> Read(string asset)
        {
            return ItemTableExtractor.Extract(new StringReader(asset));
        }

        [Fact]
        public void ReadsEveryItemsIdAndStackName()
        {
            IReadOnlyList<DialogueItem> items = Read(Asset);

            Assert.Equal(4, items.Count);
            Assert.Equal("drug_alcohol_commodore_red", items[0].Name);
            Assert.Equal(string.Empty, items[0].StackName);
            Assert.Equal("key_trash_container", items[1].Name);
            Assert.Equal("key_ring", items[1].StackName);
        }

        /// <summary>The index the database stores is mapped through the game's own table.</summary>
        [Fact]
        public void ReadsTheGroupByTheGamesNameForIt()
        {
            IReadOnlyList<DialogueItem> items = Read(Asset);

            Assert.Equal("alcohol", items[0].Group);
            Assert.Equal("none", items[1].Group);
            Assert.Equal("tare", items[2].Group);
        }

        /// <summary>
        /// A record with no itemGroup is not in any group, which is what a thought is.
        /// </summary>
        [Fact]
        public void ARecordWithNoGroupFieldReadsAsNone()
        {
            Assert.Equal("none", Read(Asset)[3].Group);
        }

        /// <summary>
        /// An index the table does not cover reads as no group rather than throwing.
        /// </summary>
        /// <remarks>
        /// Every shipped record stores a number in range, so this is about a database someone
        /// has edited. Answering "no group" is what the game's own out-of-range handling
        /// amounts to, and it keeps one bad field from failing the whole extraction.
        /// </remarks>
        [Theory]
        [InlineData("7")]
        [InlineData("-1")]
        [InlineData("")]
        [InlineData("alcohol")]
        public void AnIndexOutsideTheTableReadsAsNone(string stored)
        {
            string asset = Asset.Replace("value: 6", "value: " + stored);

            Assert.Equal("none", Read(asset)[2].Group);
        }

        [Fact]
        public void StopsAtTheSectionAfterIt()
        {
            // The section that follows carries a Name field of its own, so a reader that ran
            // past the end would report it as an item.
            Assert.DoesNotContain(Read(Asset), item => item.Name == "Not an item");
        }

        [Fact]
        public void WritesOneObjectPerLine()
        {
            var written = new StringWriter { NewLine = "\n" };
            ItemTableFile.Write(written, Read(Asset));

            Assert.Equal(
                "{\"name\":\"drug_alcohol_commodore_red\",\"stack\":\"\",\"display\":\"\","
                    + "\"group\":\"alcohol\",\"bonuses\":[]}\n"
                    + "{\"name\":\"key_trash_container\",\"stack\":\"key_ring\",\"display\":\"\","
                    + "\"group\":\"none\",\"bonuses\":[]}\n"
                    + "{\"name\":\"glass_tare\",\"stack\":\"\",\"display\":\"\","
                    + "\"group\":\"tare\",\"bonuses\":[]}\n"
                    + "{\"name\":\"revacholian_nationhood\",\"stack\":\"\",\"display\":\"\","
                    + "\"group\":\"none\",\"bonuses\":[]}\n",
                written.ToString());
        }
        /// <summary>An item stating what it moves, in the forms the database uses.</summary>
        /// <remarks>
        /// Every shape here is one the shipped database contains - see de-sr1u.2, which counted
        /// them. The flavour after the colon is the player's and is not kept.
        /// </remarks>
        private static string Stating(string text) => $$"""
              items:
              - id: 1
                fields:
                - title: Name
                  value: worn_thing
                  type: 0
                  typeString:
                - title: MediumTextValue
                  value: {{text}}
                  type: 0
                  typeString:
            """;

        [Fact]
        public void ReadsASingleStatedBonus()
        {
            ItemBonus bonus = Assert.Single(Read(Stating("'+1 Rhetoric: The heroic deeds'"))[0].Bonuses);

            Assert.Equal(1, bonus.Amount);
            Assert.Equal("Rhetoric", bonus.Moves);
        }

        /// <summary>Several bonuses are one value, joined by an escaped newline.</summary>
        [Fact]
        public void ReadsEveryBonusInOneValue()
        {
            IReadOnlyList<ItemBonus> bonuses = Read(Stating(
                @"""+1 Pain Threshold: Thicker skin\n+1 Authority: Borrowed confidence\n-2 Empathy: Numb"""))[0].Bonuses;

            Assert.Equal(3, bonuses.Count);
            Assert.Equal(new ItemBonus(1, "Pain Threshold"), bonuses[0]);
            Assert.Equal(new ItemBonus(1, "Authority"), bonuses[1]);
            Assert.Equal(new ItemBonus(-2, "Empathy"), bonuses[2]);
        }

        /// <summary><c>+1 to X when equipped</c> says what <c>+1 X</c> says.</summary>
        [Fact]
        public void ReadsTheLongerFormAsTheSameBonus()
        {
            ItemBonus bonus = Assert.Single(
                Read(Stating("'+1 to Kingdom of Conscience when equipped: You moralist douche'"))[0].Bonuses);

            Assert.Equal(new ItemBonus(1, "Kingdom of Conscience"), bonus);
        }

        /// <summary>A condition travels with the name rather than being dropped.</summary>
        /// <remarks>
        /// One bonus in the database is conditional. Keeping the parenthetical means a reader
        /// can see the condition is there; dropping it would state the bonus as unconditional,
        /// which is a claim the database does not make.
        /// </remarks>
        [Fact]
        public void KeepsAConditionWithWhatItQualifies()
        {
            ItemBonus bonus = Assert.Single(
                Read(Stating("'-1 Suggestion (unless wearing full armor): Clanking'"))[0].Bonuses);

            Assert.Equal(new ItemBonus(-1, "Suggestion (unless wearing full armor)"), bonus);
        }

        /// <summary>A non-breaking space between <c>to</c> and the name is still a space.</summary>
        /// <remarks>
        /// One bonus in the database separates them with a raw 0xA0, which is not valid UTF-8
        /// and arrives as a replacement character rather than as a space. Read as written, the
        /// word <c>to</c> travels into the name.
        /// </remarks>
        [Theory]
        [InlineData(" ")]
        [InlineData("�")]
        public void ReadsAcrossANonBreakingSpace(string gap)
        {
            ItemBonus bonus = Assert.Single(
                Read(Stating($"'+1 to{gap}Kingdom of Conscience when equipped: Douche'"))[0].Bonuses);

            Assert.Equal(new ItemBonus(1, "Kingdom of Conscience"), bonus);
        }

        /// <summary>Text that states no bonus yields none, rather than a zero.</summary>
        [Theory]
        [InlineData("'Heal all Health.'")]
        [InlineData("'Heals all health (when consumed in dialogue): Culinary resurrection'")]
        public void TextStatingNoBonusYieldsNone(string text)
        {
            Assert.Empty(Read(Stating(text))[0].Bonuses);
        }

        /// <summary>An item that states nothing carries no bonuses, which is nearly all of them.</summary>
        [Fact]
        public void AnItemWithNoStatementCarriesNoBonuses()
        {
            Assert.All(Read(Asset), item => Assert.Empty(item.Bonuses));
        }

        /// <summary>
        /// The spelling is the database's, misspellings and all - see <see cref="ItemBonus"/>.
        /// </summary>
        [Fact]
        public void KeepsTheDatabasesOwnSpelling()
        {
            ItemBonus bonus = Assert.Single(
                Read(Stating("'+1 Electrochemisty: Become an addict'"))[0].Bonuses);

            Assert.Equal("Electrochemisty", bonus.Moves);
        }
    }
}
