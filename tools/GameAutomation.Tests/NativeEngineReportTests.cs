// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Reading the plugin's native look-ahead line back out of a BepInEx log.
    /// </summary>
    /// <remarks>
    /// No game needed: the parsing is the part that can be got wrong quietly, and it can
    /// be checked against text. What cannot be checked here is whether the line ever
    /// appears, which is what the in-game run is for.
    /// </remarks>
    public class NativeEngineReportTests
    {
        /// <summary>A log from a launch where everything worked.</summary>
        private const string Working =
            "[Message:   BepInEx] Loading [GlobalConversationTracker 0.1.0]\n"
            + "[Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.\n"
            + "[Message:GlobalConversationTracker] Native look-ahead: library v0.1.0 loaded.\n"
            + "[Message:GlobalConversationTracker] Native look-ahead: index opened, "
            + "1501 conversations, 10645 declared variables.\n"
            + "[Message:GlobalConversationTracker] Global state file: C:\\saves\\gct.json\n";

        [Fact]
        public void AWorkingLaunchReportsAVersionAndACount()
        {
            NativeEngineReport report = NativeEngineReport.FromText(Working);

            Assert.True(report.Loaded);
            Assert.Equal("0.1.0", report.Version);
            Assert.Equal(1501, report.Conversations);
            Assert.Equal(10645, report.Variables);
        }

        /// <summary>
        /// An index opened with no variable table beside it reports zero, not "no index".
        /// </summary>
        /// <remarks>
        /// The two mean quite different things, and only one of them is a problem. Zero
        /// declared variables is a mod that works and answers unset dialogue variables less
        /// precisely - which nothing would ever notice, which is why it is reported.
        /// </remarks>
        [Fact]
        public void AnIndexWithNoVariableTableReportsZeroRatherThanNothing()
        {
            NativeEngineReport report = NativeEngineReport.FromText(
                "[Message:GlobalConversationTracker] Native look-ahead: library v0.1.0 loaded.\n"
                + "[Message:GlobalConversationTracker] Native look-ahead: index opened, "
                + "1501 conversations, 0 declared variables.\n");

            Assert.True(report.Loaded);
            Assert.Equal(1501, report.Conversations);
            Assert.Equal(0, report.Variables);
        }

        /// <summary>
        /// The library loaded but no index was deployed beside it - which is the state
        /// this repository is in until de-i5xj.3, so it must read as success with no count
        /// rather than as a failure.
        /// </summary>
        [Fact]
        public void ALoadWithNoIndexIsStillALoad()
        {
            NativeEngineReport report = NativeEngineReport.FromText(
                "[Message:GlobalConversationTracker] Native look-ahead: library v0.1.0 loaded.\n"
                + "[Message:GlobalConversationTracker] Native look-ahead: no "
                + "conversation_index.jsonl beside the plugin; nothing to open yet.\n");

            Assert.True(report.Loaded);
            Assert.Equal("0.1.0", report.Version);
            Assert.Equal(-1, report.Conversations);
        }

        /// <summary>
        /// A missing library is the failure this whole exercise exists to catch, and the
        /// reported line has to carry the reason - the exception type is what says whether
        /// the DLL was absent or the wrong architecture.
        /// </summary>
        [Fact]
        public void AMissingLibraryIsNotLoadedAndKeepsTheReason()
        {
            NativeEngineReport report = NativeEngineReport.FromText(
                "[Warning:GlobalConversationTracker] Native look-ahead: unavailable "
                + "(DllNotFoundException: Unable to load DLL "
                + "'GlobalConversationTracker.Native'). The look-ahead will use the "
                + "managed engine.\n");

            Assert.False(report.Loaded);
            Assert.Null(report.Version);
            Assert.Contains("DllNotFoundException", report.ToString());
        }

        /// <summary>A log that never mentions it says so, rather than looking like a failure.</summary>
        [Fact]
        public void ALogThatNeverMentionsItSaysSo()
        {
            NativeEngineReport report = NativeEngineReport.FromText(
                "[Message:   BepInEx] Chainloader startup complete\n");

            Assert.False(report.Loaded);
            Assert.Null(report.Line);
            Assert.Contains("said nothing", report.ToString());
        }

        [Fact]
        public void AMissingLogIsNotALoad()
        {
            NativeEngineReport report = NativeEngineReport.FromLog("no-such-log.txt");

            Assert.False(report.Loaded);
            Assert.Null(report.Line);
        }
    }
}
