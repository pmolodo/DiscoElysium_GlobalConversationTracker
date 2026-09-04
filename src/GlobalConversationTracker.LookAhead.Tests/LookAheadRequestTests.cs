// SPDX-License-Identifier: MIT
using System.Text.Json;
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// What a request puts on the wire.
    /// </summary>
    /// <remarks>
    /// The budgets specifically. A budget the plugin holds and the engine never hears about
    /// is a dial connected to nothing, and that is not a hypothetical: it is exactly what
    /// happened when the marker moved to the bridge and the state budget was left behind -
    /// the in-game suite that sets a budget of one still marked three options, because the
    /// engine doing the searching had never been told. See de-i5xj.6.
    /// </remarks>
    public class LookAheadRequestTests
    {
        private static JsonElement Sent(LookAheadRequest request) =>
            JsonDocument.Parse(request.ToJson()).RootElement;

        private static LookAheadRequest Request() =>
            new LookAheadRequest(1, new WorldSnapshot());

        /// <summary>Every budget crosses, under the name the engine reads.</summary>
        [Fact]
        public void TheBudgetsCross()
        {
            LookAheadRequest request = Request();
            request.StateBudget = 7;
            request.TimeBudgetMs = 250;
            request.MemoryBudgetMb = 64;

            JsonElement sent = Sent(request);
            Assert.Equal(7, sent.GetProperty("state_budget").GetInt32());
            Assert.Equal(250, sent.GetProperty("time_budget_ms").GetInt32());
            Assert.Equal(64, sent.GetProperty("memory_budget_mb").GetInt32());
        }

        /// <summary>
        /// An unset budget crosses as zero rather than not at all.
        /// </summary>
        /// <remarks>
        /// Zero is a value the engine understands - the default for memory and states, no
        /// limit for time - so sending it says what the plugin means. Omitting the field
        /// would mean the same thing only by accident of the deserialiser's defaults, and
        /// would stop saying it the moment one of them changed.
        /// </remarks>
        [Fact]
        public void AnUnsetBudgetStillCrosses()
        {
            JsonElement sent = Sent(Request());

            Assert.Equal(0, sent.GetProperty("state_budget").GetInt32());
            Assert.Equal(0, sent.GetProperty("time_budget_ms").GetInt32());
            Assert.Equal(0, sent.GetProperty("memory_budget_mb").GetInt32());
        }

        /// <summary>A negative budget crosses as zero, not as a negative.</summary>
        /// <remarks>
        /// A player can type anything into a configuration file. Zero is "use the default",
        /// which is the safe reading of a number that cannot mean what it says; passing the
        /// negative through would have the engine compare an unsigned size against it.
        /// </remarks>
        [Fact]
        public void ANegativeBudgetCrossesAsZero()
        {
            LookAheadRequest request = Request();
            request.StateBudget = -1;
            request.TimeBudgetMs = -1;
            request.MemoryBudgetMb = -1;

            JsonElement sent = Sent(request);
            Assert.Equal(0, sent.GetProperty("state_budget").GetInt32());
            Assert.Equal(0, sent.GetProperty("time_budget_ms").GetInt32());
            Assert.Equal(0, sent.GetProperty("memory_budget_mb").GetInt32());
        }
    }
}
