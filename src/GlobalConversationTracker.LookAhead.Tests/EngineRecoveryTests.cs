// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Does the recovery policy blame the right thing, and stop at the right time?
    /// </summary>
    /// <remarks>
    /// <para>de-bnjy.1.3. <see cref="EngineRecovery"/> is pure bookkeeping precisely so
    /// these questions can be asked without a game: every branch below is reached by
    /// telling it about deaths that never happened.</para>
    ///
    /// <para>THE ONE THAT MATTERS IS THE FIRST. The only failure of this kind ever observed
    /// here was cumulative rather than a property of the group (de-fpax), and a policy that
    /// quarantined on a single death would have condemned a healthy conversation for it.
    /// So the test that a first death blames nobody is the test that this class earns its
    /// existence.</para>
    /// </remarks>
    public class EngineRecoveryTests
    {
        /// <summary>Two conversations that are not each other; nothing else about them matters.</summary>
        private const int SomeConversation = 28;
        private const int OtherConversation = 631;

        [Fact]
        public void FreshPolicy_BlamesNobodyAndHasNotGivenUp()
        {
            var recovery = new EngineRecovery();

            Assert.Equal(0, recovery.Deaths);
            Assert.Equal(0, recovery.Respawns);
            Assert.False(recovery.GivenUp);
            Assert.Empty(recovery.Quarantined);
            Assert.False(recovery.IsQuarantined(SomeConversation));
        }

        /// <summary>A single death respawns and convicts nothing.</summary>
        /// <remarks>
        /// de-fpax is the whole reason. Conversation 28's row overflowed the stack as the
        /// THIRD row of a run and finished in 58 ms in a process of its own, so what killed
        /// that engine was the run rather than the row. One death is not evidence.
        /// </remarks>
        [Fact]
        public void FirstDeath_RespawnsWithoutQuarantining()
        {
            var recovery = new EngineRecovery();

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(SomeConversation));

            Assert.Equal(1, recovery.Deaths);
            Assert.Equal(1, recovery.Respawns);
            Assert.False(recovery.IsQuarantined(SomeConversation));
        }

        /// <summary>The same group killing a FRESH engine too is what convicts it.</summary>
        /// <remarks>
        /// The second death happened in an engine started after the first, so the
        /// accumulation a cumulative fault needs was not there to do it - which leaves the
        /// group.
        /// </remarks>
        [Fact]
        public void SameGroupTwice_IsQuarantined()
        {
            var recovery = new EngineRecovery();

            recovery.RecordDeath(SomeConversation);

            Assert.Equal(
                RecoveryAction.QuarantineAndRespawn,
                recovery.RecordDeath(SomeConversation));

            Assert.True(recovery.IsQuarantined(SomeConversation));
            Assert.Equal(new[] { SomeConversation }, recovery.Quarantined);
        }

        /// <summary>An answer from the suspect clears the suspicion, so it cannot be convicted later.</summary>
        /// <remarks>
        /// THIS IS WHAT MAKES THE QUARANTINE PROVISIONAL, and it is the cumulative fault's
        /// exit: a group that killed an engine and then answered in the fresh one has
        /// proved the fault was not its own, and a death much later must not reach back and
        /// use the first one as half the evidence.
        /// </remarks>
        [Fact]
        public void SuspectThatAnswers_IsNotConvictedByALaterDeath()
        {
            var recovery = new EngineRecovery();

            recovery.RecordDeath(SomeConversation);
            recovery.RecordAnswered(SomeConversation);

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(SomeConversation));
            Assert.False(recovery.IsQuarantined(SomeConversation));
        }

        /// <summary>An answer from some OTHER conversation leaves the suspicion standing.</summary>
        /// <remarks>
        /// Only the suspect can clear itself. Otherwise the first menu drawn anywhere after
        /// a death would exonerate whatever caused it, and nothing would ever be convicted.
        /// </remarks>
        [Fact]
        public void AnswerFromAnotherConversation_DoesNotClearTheSuspicion()
        {
            var recovery = new EngineRecovery();

            recovery.RecordDeath(SomeConversation);
            recovery.RecordAnswered(OtherConversation);

            Assert.Equal(
                RecoveryAction.QuarantineAndRespawn,
                recovery.RecordDeath(SomeConversation));
        }

        /// <summary>Deaths on different groups convict neither of them.</summary>
        /// <remarks>
        /// Two groups dying in turn is the signature of a fault that belongs to neither -
        /// which is the cumulative case again, seen from a different angle than the
        /// answered-suspect one.
        /// </remarks>
        [Fact]
        public void DeathsOnDifferentGroups_QuarantineNothing()
        {
            var recovery = new EngineRecovery();

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(SomeConversation));
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(OtherConversation));
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(SomeConversation));

            Assert.Empty(recovery.Quarantined);
        }

        /// <summary>A death with nothing in flight counts, and suspects nobody.</summary>
        /// <remarks>
        /// It also CLEARS a standing suspicion, deliberately: an engine that died with no
        /// request to blame died of something that was not a request, which is evidence for
        /// the cumulative fault and against the group that happened to be suspected.
        /// </remarks>
        [Fact]
        public void DeathWithNothingInFlight_OnlyCounts()
        {
            var recovery = new EngineRecovery();

            recovery.RecordDeath(SomeConversation);

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(null));
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(SomeConversation));

            Assert.Equal(3, recovery.Deaths);
            Assert.Empty(recovery.Quarantined);
        }

        /// <summary>A replacement that cannot start counts against the same budget.</summary>
        /// <remarks>
        /// Or a machine that cannot start the engine at all would be asked to for ever and
        /// the give-up would never fire.
        /// </remarks>
        [Fact]
        public void StartupFailure_CountsAndReachesTheLimit()
        {
            var recovery = new EngineRecovery(limit: 2);

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordStartupFailure());
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordStartupFailure());
            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordStartupFailure());

            Assert.True(recovery.GivenUp);
        }

        /// <summary>A failed startup suspects nobody, and does not exonerate anybody either.</summary>
        /// <remarks>
        /// A process that never came up cannot have been killed by a conversation - but the
        /// suspect has not been retried yet either, so the suspicion has to survive the
        /// failed attempt to retry it.
        /// </remarks>
        [Fact]
        public void StartupFailure_LeavesTheSuspicionUntouched()
        {
            var recovery = new EngineRecovery();

            recovery.RecordDeath(SomeConversation);
            recovery.RecordStartupFailure();

            Assert.Equal(
                RecoveryAction.QuarantineAndRespawn,
                recovery.RecordDeath(SomeConversation));
        }

        /// <summary>The limit tolerates that many deaths, and gives up on the next one.</summary>
        /// <remarks>
        /// Counted rather than rated, and so deterministic - which is the only reason a
        /// test can say this at all. The five of <see cref="EngineRecovery.DefaultLimit"/>
        /// is written small here so the test does not turn on the number.
        /// </remarks>
        [Fact]
        public void Limit_IsHowManyDeathsAreTolerated()
        {
            var recovery = new EngineRecovery(limit: 3);

            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(null));
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(null));
            Assert.Equal(RecoveryAction.Respawn, recovery.RecordDeath(null));
            Assert.False(recovery.GivenUp);

            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordDeath(null));
            Assert.True(recovery.GivenUp);
            Assert.Equal(3, recovery.Respawns);
        }

        /// <summary>Having given up, it stays given up.</summary>
        /// <remarks>
        /// A caller that keeps asking after being told to stop gets told to stop, rather
        /// than restarting a budget it has already spent.
        /// </remarks>
        [Fact]
        public void GivingUp_IsFinal()
        {
            var recovery = new EngineRecovery(limit: 1);

            recovery.RecordDeath(null);
            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordDeath(null));

            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordDeath(SomeConversation));
            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordDeath(SomeConversation));
            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordStartupFailure());

            // The second of those two deaths on the same conversation would have convicted
            // it, had the policy still been listening. Once it has stopped, it stops
            // deciding anything at all.
            Assert.Empty(recovery.Quarantined);
            Assert.Equal(1, recovery.Respawns);
        }

        /// <summary>A limit of zero never respawns, which is the behaviour this replaced.</summary>
        /// <remarks>
        /// Kept reachable so a test can get to the give-up path in one death rather than
        /// six. It is also, exactly, what de-bnjy.1.2 shipped.
        /// </remarks>
        [Fact]
        public void LimitOfZero_GivesUpOnTheFirstDeath()
        {
            var recovery = new EngineRecovery(limit: 0);

            Assert.Equal(RecoveryAction.GiveUp, recovery.RecordDeath(SomeConversation));

            Assert.True(recovery.GivenUp);
            Assert.Equal(0, recovery.Respawns);
            Assert.Empty(recovery.Quarantined);
        }

        /// <summary>A negative limit is refused rather than quietly meaning something.</summary>
        [Fact]
        public void NegativeLimit_IsRejected()
        {
            Assert.Throws<ArgumentOutOfRangeException>(() => new EngineRecovery(limit: -1));
        }
    }
}
