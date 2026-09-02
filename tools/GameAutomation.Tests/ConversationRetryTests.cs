// SPDX-License-Identifier: MIT
using System;
using System.Text.Json;
using GlobalConversationTracker.Harness;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The decisions behind re-asking for a conversation that did not open.
    /// </summary>
    /// <remarks>
    /// The retry loop itself needs a game - it presses Enter at a real window - but the
    /// two judgements it turns on do not, and they are the parts that are easy to get
    /// subtly wrong: reading a missing diagnostic as a failure, and letting retries
    /// multiply how long a stuck scenario takes to give up.
    /// </remarks>
    public class ConversationRetryTests
    {
        private static ProbeEvent Answer(string json) =>
            new ProbeEvent(JsonDocument.Parse(json).RootElement);

        [Fact]
        public void FellStraightOut_IsTrueWhenTheProbeSaysNothingIsRunning()
        {
            Assert.True(FellStraightOut(
                "{\"event\":\"command-finished\",\"command\":\"start-conversation\","
                + "\"active\":false}"));
        }

        [Fact]
        public void FellStraightOut_IsFalseWhenTheConversationIsRunning()
        {
            Assert.False(FellStraightOut(
                "{\"event\":\"command-finished\",\"command\":\"start-conversation\","
                + "\"active\":true}"));
        }

        /// <summary>
        /// "Do not know" must not be read as "failed". The probe returns no answer when
        /// it could not ask the dialogue system, and retrying on that would eventually
        /// fail scenarios that were about to draw their menu.
        /// </summary>
        [Theory]
        [InlineData("{\"event\":\"command-finished\",\"command\":\"start-conversation\"}")]
        [InlineData("{\"event\":\"command-finished\",\"active\":null}")]
        [InlineData("{\"event\":\"command-finished\",\"active\":\"false\"}")]
        public void FellStraightOut_IsFalseWhenTheProbeDidNotSay(string json)
        {
            Assert.False(FellStraightOut(json));
        }

        [Fact]
        public void FellStraightOut_RejectsNull()
        {
            Assert.Throws<ArgumentNullException>(() => LookAheadRun.FellStraightOut(null!));
        }

        /// <summary>
        /// Retrying must not multiply the time a stuck scenario takes to report, so the
        /// attempts divide the scenario's timeout rather than each taking all of it.
        /// </summary>
        [Fact]
        public void AttemptTimeout_GivesOneAttemptLessThanTheWholeScenarioTimeout()
        {
            TimeSpan total = TimeSpan.FromSeconds(300);

            TimeSpan attempt = LookAheadRun.AttemptTimeout(total);

            Assert.True(attempt > TimeSpan.Zero, "an attempt must get some time");
            Assert.True(
                attempt < total,
                $"one attempt got {attempt}, the whole {total} scenario timeout, so "
                + "retrying would multiply how long a stuck scenario takes to report");
        }

        /// <summary>
        /// A share, not a fixed budget: doubling what a scenario is allowed doubles what
        /// each of its attempts is allowed.
        /// </summary>
        [Fact]
        public void AttemptTimeout_IsProportionalToTheScenarioTimeout()
        {
            TimeSpan single = LookAheadRun.AttemptTimeout(TimeSpan.FromSeconds(300));
            TimeSpan doubled = LookAheadRun.AttemptTimeout(TimeSpan.FromSeconds(600));

            Assert.Equal(single + single, doubled);
        }

        private static bool FellStraightOut(string json) =>
            LookAheadRun.FellStraightOut(Answer(json));
    }
}
