using System;
using GlobalConversationTracker;
using Xunit;

namespace GlobalConversationTracker.Tests
{
    public class SimStatusNamesTests
    {
        [Theory]
        [InlineData("Untouched", SimStatus.Untouched)]
        [InlineData("WasOffered", SimStatus.WasOffered)]
        [InlineData("WasDisplayed", SimStatus.WasDisplayed)]
        public void TryParse_RecognizesEveryGameString(string name, SimStatus expected)
        {
            Assert.True(SimStatusNames.TryParse(name, out SimStatus status));
            Assert.Equal(expected, status);
            Assert.Equal(expected, SimStatusNames.Parse(name));
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData(" ")]
        [InlineData("Bogus")]
        [InlineData("wasdisplayed")]      // the game never lower-cases it
        [InlineData("WASDISPLAYED")]
        [InlineData(" WasDisplayed")]     // no trimming: the game writes it exactly
        [InlineData("WasDisplayed ")]
        [InlineData("WasSpoken")]         // plausible-looking but not a real status
        [InlineData("2")]                 // the numeric enum value is not a game string
        public void TryParse_RejectsUnrecognizedStatusStrings(string? name)
        {
            Assert.False(SimStatusNames.TryParse(name, out SimStatus status));
            Assert.Equal(SimStatus.Untouched, status);
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("Bogus")]
        [InlineData("wasdisplayed")]
        public void Parse_ThrowsOnUnrecognizedStatusString(string? name)
        {
            ArgumentException ex = Assert.Throws<ArgumentException>(() => SimStatusNames.Parse(name));
            Assert.Equal("name", ex.ParamName);
            // The offending value has to be in the message so a mod log is actionable.
            Assert.Contains(name is null ? "<null>" : $"'{name}'", ex.Message);
        }

        [Theory]
        [InlineData(SimStatus.Untouched, "Untouched")]
        [InlineData(SimStatus.WasOffered, "WasOffered")]
        [InlineData(SimStatus.WasDisplayed, "WasDisplayed")]
        public void ToGameString_RoundTrips(SimStatus status, string expected)
        {
            string name = SimStatusNames.ToGameString(status);
            Assert.Equal(expected, name);
            Assert.Equal(status, SimStatusNames.Parse(name));
        }

        [Fact]
        public void ToGameString_ThrowsOnUndefinedEnumValue()
        {
            Assert.Throws<ArgumentOutOfRangeException>(() => SimStatusNames.ToGameString((SimStatus)99));
        }

        [Fact]
        public void SimStatus_OrderingIsUntouchedThenOfferedThenDisplayed()
        {
            // The merge rule is implemented as a numeric comparison, so the numbering
            // is load bearing.
            Assert.True(SimStatus.Untouched < SimStatus.WasOffered);
            Assert.True(SimStatus.WasOffered < SimStatus.WasDisplayed);
            Assert.Equal(0, (int)SimStatus.Untouched);
            Assert.Equal(3, Enum.GetValues(typeof(SimStatus)).Length);
        }
    }
}
