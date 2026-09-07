// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>What to do about an engine that has just died.</summary>
    public enum RecoveryAction
    {
        /// <summary>Start a fresh engine and carry on. Nothing is blamed.</summary>
        Respawn,

        /// <summary>
        /// Start a fresh engine, and stop asking about the group that killed this one.
        /// </summary>
        /// <remarks>
        /// Only after that group has killed a FRESH engine too - see
        /// <see cref="EngineRecovery"/> for why one death is not evidence against a group.
        /// </remarks>
        QuarantineAndRespawn,

        /// <summary>Stop. Too many engines have died to keep paying for another.</summary>
        GiveUp,
    }

    /// <summary>
    /// Whether to respawn a dead look-ahead engine, whether to blame the group that killed
    /// it, and when to stop trying.
    /// </summary>
    /// <remarks>
    /// <para>de-bnjy.1.3. Before this, one death ended the look-ahead for the session
    /// (de-bnjy.1.2) - the right first behaviour and the wrong last one.</para>
    ///
    /// <para>## Why a quarantine alone would be wrong, and it is measured</para>
    ///
    /// <para>THE ONE FAILURE OF THIS KIND ACTUALLY OBSERVED HERE WAS NOT A PROPERTY OF THE
    /// GROUP. de-fpax: conversation 28's row overflowed the stack as the THIRD row of a run,
    /// and running the same row in its own process finished in 58 ms. What accumulated was
    /// per-thread state inside the diagram manager, not anything about that conversation.</para>
    ///
    /// <para>So the two halves of the obvious response suit opposite faults. A RESPAWN is
    /// exactly right for a cumulative one - a fresh process is precisely what resets the
    /// accumulation. A QUARANTINE is exactly wrong for it: it condemns a group that would
    /// work perfectly in the new process, while the real cause goes on accumulating and
    /// takes down something else next. Together, under a cumulative fault, they would
    /// blacklist a healthy group per crash AND still reach the give-up count - worse than
    /// either alone.</para>
    ///
    /// <para>## So the quarantine is PROVISIONAL</para>
    ///
    /// <para>A group that kills an engine is only SUSPECTED. It is retried in the fresh
    /// process, and only if it kills that one too is it quarantined. That distinguishes the
    /// two faults with evidence already in hand and costs one extra attempt rather than a
    /// new mechanism: a cumulative fault clears the suspicion the moment the group answers,
    /// and a genuinely poisonous group cannot answer and so convicts itself.</para>
    ///
    /// <para>## The counter is an absolute count, per session</para>
    ///
    /// <para>Five, and counted rather than rated. A rate is the more honest reading of
    /// "something is badly wrong NOW" - five deaths in a minute is not five spread over an
    /// afternoon - but an absolute count is deterministic, which means an in-game suite can
    /// assert it, and the cost of being wrong is small: by the fifth crash the player has
    /// had five crashes either way. Deterministic and slightly pessimistic beats honest and
    /// untestable here.</para>
    ///
    /// <para>A RESPAWN THAT DIES ON STARTUP COUNTS. Otherwise an engine that cannot come up
    /// at all would be retried for ever and the give-up would never fire; that is what
    /// <see cref="RecordStartupFailure"/> is for.</para>
    ///
    /// <para>Pure bookkeeping, no processes and no IO, so the policy is testable without a
    /// game - which is the point of it living here rather than in the patch that applies
    /// it.</para>
    /// </remarks>
    public sealed class EngineRecovery
    {
        /// <summary>How many engines may die before the look-ahead gives up for good.</summary>
        public const int DefaultLimit = 5;

        private readonly HashSet<int> _quarantined = new HashSet<int>();
        private readonly int _limit;

        /// <summary>
        /// The conversation that killed the last engine, or null if none has or the last
        /// suspect has since answered.
        /// </summary>
        private int? _suspect;

        /// <summary>Creates a recovery policy.</summary>
        /// <param name="limit">
        /// How many deaths to tolerate; <see cref="DefaultLimit"/> when not given. Must be
        /// at least one, since a limit of zero would give up before the first respawn and
        /// is better spelled by not using this at all.
        /// </param>
        /// <exception cref="ArgumentOutOfRangeException"><paramref name="limit"/> is below one.</exception>
        public EngineRecovery(int limit = DefaultLimit)
        {
            if (limit < 1)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(limit),
                    limit,
                    "a recovery limit below one gives up before it has tried anything");
            }

            _limit = limit;
        }

        /// <summary>How many engines have died this session.</summary>
        public int Deaths { get; private set; }

        /// <summary>How many of those deaths were answered by starting a fresh engine.</summary>
        public int Respawns { get; private set; }

        /// <summary>Whether the look-ahead has given up for the session.</summary>
        public bool GivenUp { get; private set; }

        /// <summary>The conversations that are no longer asked about.</summary>
        public IReadOnlyCollection<int> Quarantined => _quarantined;

        /// <summary>Whether this conversation has been convicted of killing engines.</summary>
        /// <remarks>
        /// A caller must not ask the engine about a quarantined conversation. What it
        /// should draw instead is the uncertain marker - a search really did run and really
        /// did not finish, which is exactly what that marker means (de-pvq). Drawing
        /// nothing would say "there is nothing here", which is a different and false claim.
        /// </remarks>
        public bool IsQuarantined(int conversation) => _quarantined.Contains(conversation);

        /// <summary>Records that an engine died while working on a conversation.</summary>
        /// <param name="conversation">
        /// The conversation whose request was in flight, or null where nothing was - a death
        /// with no request to blame suspects nothing and only counts.
        /// </param>
        /// <returns>What to do about it.</returns>
        public RecoveryAction RecordDeath(int? conversation)
        {
            Deaths++;

            // ALREADY GIVEN UP stays given up. A caller that keeps asking after being told
            // to stop gets told to stop, rather than restarting the count.
            if (GivenUp || Deaths > _limit)
            {
                GivenUp = true;
                return RecoveryAction.GiveUp;
            }

            Respawns++;

            // TWICE IN A ROW IS THE EVIDENCE. The second death happened in an engine started
            // after the first, so the accumulation a cumulative fault needs was not there -
            // which leaves the group itself.
            if (conversation.HasValue && _suspect == conversation.Value)
            {
                _quarantined.Add(conversation.Value);
                _suspect = null;
                return RecoveryAction.QuarantineAndRespawn;
            }

            _suspect = conversation;
            return RecoveryAction.Respawn;
        }

        /// <summary>
        /// Records that a replacement engine could not be started at all.
        /// </summary>
        /// <returns>Whether to keep trying.</returns>
        /// <remarks>
        /// COUNTED AGAINST THE SAME BUDGET, or a machine that cannot start the engine would
        /// be asked to for ever. It suspects nothing: a process that never came up cannot
        /// have been killed by a conversation.
        /// </remarks>
        public RecoveryAction RecordStartupFailure()
        {
            Deaths++;
            if (GivenUp || Deaths > _limit)
            {
                GivenUp = true;
                return RecoveryAction.GiveUp;
            }

            Respawns++;
            return RecoveryAction.Respawn;
        }

        /// <summary>
        /// Records that a conversation was answered, which clears any suspicion of it.
        /// </summary>
        /// <remarks>
        /// THIS IS WHAT MAKES THE QUARANTINE PROVISIONAL. A group that killed an engine and
        /// then answered in the fresh one has proved the fault was cumulative rather than
        /// its own, so it must not be convicted if something else dies later.
        /// </remarks>
        public void RecordAnswered(int conversation)
        {
            if (_suspect == conversation)
            {
                _suspect = null;
            }
        }
    }
}
