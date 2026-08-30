// SPDX-License-Identifier: MIT
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class PassiveCheckTests
    {
        /// <summary>
        /// The comparison is <c>skill + 6 >= threshold</c>, so the boundary sits where
        /// the bonus exactly closes the gap - an off-by-one here would silently move
        /// every marker in the game.
        /// </summary>
        [Theory]
        [InlineData(0, 6, true)]
        [InlineData(0, 7, false)]
        [InlineData(4, 10, true)]
        [InlineData(4, 11, false)]
        [InlineData(12, 12, true)]
        public void Clears_AppliesTheFlatSixBonus(int skill, int threshold, bool expected)
        {
            Assert.Equal(expected, PassiveCheck.Clears(skill, threshold));
        }

        [Fact]
        public void SkillBonus_IsSix()
        {
            Assert.Equal(6, PassiveCheck.SkillBonus);
        }

        [Theory]
        [InlineData(10, 8, Ternary.True)]
        [InlineData(1, 20, Ternary.False)]
        public void Outcome_FiresWhenTheCheckClears(int skill, int threshold, Ternary expected)
        {
            Assert.Equal(expected, PassiveCheck.Outcome(skill, threshold, antipassive: false));
        }

        /// <summary>
        /// An antipassive entry is the line for failing, so it fires on exactly the cases
        /// an ordinary one does not. 213 entries in the database carry the field.
        /// </summary>
        [Theory]
        [InlineData(10, 8, Ternary.False)]
        [InlineData(1, 20, Ternary.True)]
        public void Outcome_IsInvertedForAntipassive(int skill, int threshold, Ternary expected)
        {
            Assert.Equal(expected, PassiveCheck.Outcome(skill, threshold, antipassive: true));
        }

        [Theory]
        [InlineData(0, 0)]
        [InlineData(5, 11)]
        [InlineData(3, 100)]
        public void Outcome_AntipassiveIsAlwaysTheOppositeOfPassive(int skill, int threshold)
        {
            Ternary passive = PassiveCheck.Outcome(skill, threshold, antipassive: false);
            Ternary anti = PassiveCheck.Outcome(skill, threshold, antipassive: true);
            Assert.NotEqual(passive, anti);
            Assert.NotEqual(Ternary.Unknown, passive);
            Assert.NotEqual(Ternary.Unknown, anti);
        }
    }
}
