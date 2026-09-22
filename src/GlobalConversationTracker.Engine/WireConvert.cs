// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Linq;

using Wire = GlobalConversationTracker.Engine.Wire;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Between the generated wire types and this assembly's own.
    /// </summary>
    /// <remarks>
    /// <para>THERE ARE TWO SETS OF TYPES ON PURPOSE. The <c>Wire</c> namespace is generated
    /// from <c>proto/engine.proto</c> and is shaped by what encodes well: an entry set is a
    /// list of runs, an unanswered question is an unset field, a message member is nullable
    /// whether or not the thing is optional. The types beside this file are shaped by what
    /// the plugin needs to say: a set you can add to and test membership in, a value that
    /// answers "unknown" to everything.</para>
    ///
    /// <para>This is the seam, and it is the only place either shape has to know about the
    /// other. The Rust side has the same seam for the same reason, in
    /// <c>src/wire_convert.rs</c>, and the two are mirror images.</para>
    ///
    /// <para>ABSENCE IS NOT AN ERROR HERE. A protobuf message with a member missing is a
    /// valid message, so null arrives constantly and legitimately - a response with no
    /// error, a snapshot with no checks - and every conversion reads it as the empty or
    /// default value rather than refusing it.</para>
    /// </remarks>
    public static class WireConvert
    {
        /// <summary>One entry, as it crosses.</summary>
        public static Wire.NodeRef Write(NodeRef node) =>
            new Wire.NodeRef { Conversation = node.Conversation, Entry = node.Entry };

        /// <summary>And back.</summary>
        public static NodeRef Read(Wire.NodeRef node) =>
            new NodeRef(node.Conversation, node.Entry);

        /// <summary>
        /// Collapses a set into runs, the way it crosses.
        /// </summary>
        /// <remarks>
        /// Sorted by conversation and then by entry, so one set has one encoding. It has
        /// to: the in-game suites compare what crossed across runs, and a set whose bytes
        /// depended on a hash order would differ from itself.
        /// </remarks>
        public static Wire.NodeSet Write(NodeSet set)
        {
            var written = new Wire.NodeSet();
            if (set == null)
            {
                return written;
            }

            foreach (IGrouping<int, NodeRef> conversation in set
                .GroupBy(node => node.Conversation)
                .OrderBy(group => group.Key))
            {
                var runs = new Wire.ConversationRuns { Conversation = conversation.Key };
                foreach (Wire.NodeRun run in Collapse(
                    conversation.Select(node => node.Entry).OrderBy(entry => entry)))
                {
                    runs.Runs.Add(run);
                }

                written.Conversations.Add(runs);
            }

            return written;
        }

        /// <summary>Expands the runs of a set.</summary>
        /// <exception cref="FormatException">A run ends before it starts.</exception>
        public static NodeSet Read(Wire.NodeSet? set)
        {
            var nodes = new NodeSet();
            if (set == null)
            {
                return nodes;
            }

            foreach (Wire.ConversationRuns conversation in set.Conversations)
            {
                foreach (Wire.NodeRun run in conversation.Runs)
                {
                    if (run.Last < run.First)
                    {
                        throw new FormatException(
                            $"conversation {conversation.Conversation}: the run "
                            + $"{run.First}-{run.Last} runs backwards");
                    }

                    for (int entry = run.First; entry <= run.Last; entry++)
                    {
                        nodes.Add(new NodeRef(conversation.Conversation, entry));
                    }
                }
            }

            return nodes;
        }

        /// <summary>Consecutive ids, as runs. The entries ascend and do not repeat.</summary>
        private static IEnumerable<Wire.NodeRun> Collapse(IEnumerable<int> entries)
        {
            int? first = null;
            int last = 0;
            foreach (int entry in entries)
            {
                if (first == null)
                {
                    first = entry;
                    last = entry;
                    continue;
                }

                if (entry == last + 1)
                {
                    last = entry;
                    continue;
                }

                yield return new Wire.NodeRun { First = first.Value, Last = last };
                first = entry;
                last = entry;
            }

            if (first != null)
            {
                yield return new Wire.NodeRun { First = first.Value, Last = last };
            }
        }

        /// <summary>One answer to something the world was asked.</summary>
        /// <remarks>
        /// An unknowable value is an unset oneof, which is what silence means and what a
        /// default-constructed message already is.
        /// </remarks>
        public static Wire.WireValue Write(WireValue value)
        {
            if (value.IsUnknown)
            {
                return new Wire.WireValue();
            }

            return value.Written();
        }

        /// <summary>The player's situation, as the plugin sees it.</summary>
        public static Wire.WorldRawData Write(WorldRawData world)
        {
            var written = new Wire.WorldRawData
            {
                Money = world.Money,
                DayMinutes = world.DayMinutes,
                DayCounter = world.DayCounter,
                ClockLocked = world.ClockLocked,
                RedChecksFail = world.RedChecksFail,
                ChecksPass = Write(world.ChecksPass),
                ChecksFail = Write(world.ChecksFail),
                Seen = Write(world.Seen),
            };

            foreach (WireValue value in world.VariableValues)
            {
                written.VariableValues.Add(Write(value));
            }

            foreach (WireValue value in world.QueryValues)
            {
                written.QueryValues.Add(Write(value));
            }

            foreach (DataAnswer answer in world.DataValues)
            {
                var read = new Wire.DataAnswer { Read = answer.Read };
                if (!answer.Value.IsUnknown)
                {
                    read.Value = Write(answer.Value);
                }

                read.Names.AddRange(answer.Names);
                written.DataValues.Add(read);
            }

            written.Items.AddRange(world.Items);
            written.Thoughts.AddRange(world.Thoughts);
            written.FailedWhiteChecks.AddRange(world.FailedWhiteChecks);
            foreach (CheckMargin margin in world.CheckMargins)
            {
                written.CheckMargins.Add(new Wire.CheckMargin
                {
                    Node = Write(margin.Node),
                    Skill = margin.Skill,
                    Margin = margin.Margin,
                });
            }

            return written;
        }

        /// <summary>A whole menu's worth of question.</summary>
        public static Wire.LookAheadRequest Write(LookAheadRequest request)
        {
            var written = new Wire.LookAheadRequest
            {
                Conversation = request.Conversation,
                SeenAnyGame = Write(request.SeenAnyGame),
                StateBudget = (ulong)Math.Max(0, request.StateBudget),
                TimeBudgetMs = (ulong)Math.Max(0, request.TimeBudgetMs),
                MenuTimeBudgetMs = (ulong)Math.Max(0, request.MenuTimeBudgetMs),
                MemoryBudgetMb = (ulong)Math.Max(0, request.MemoryBudgetMb),
                World = Write(request.World),
            };

            foreach (NodeRef start in request.Starts)
            {
                written.Starts.Add(Write(start));
            }

            foreach (NodeRef entry in request.Encountered)
            {
                written.Encountered.Add(Write(entry));
            }

            return written;
        }

        /// <summary>Which outcome of a rolled check an answer is about.</summary>
        /// <remarks>
        /// NONE IS AN ORDINARY OPTION and comes back as null, which is how the mod decides
        /// whether to draw the Pass / Fail line under it at all.
        /// </remarks>
        private static string? BranchOf(Wire.Branch branch) => branch switch
        {
            Wire.Branch.Pass => LookAheadAnswer.Pass,
            Wire.Branch.Fail => LookAheadAnswer.Fail,
            _ => null,
        };

        /// <summary>What stopped a search, as the diagnostics spell it.</summary>
        private static string StoppedByOf(Wire.StoppedBy stopped) => stopped switch
        {
            Wire.StoppedBy.States => "states",
            Wire.StoppedBy.Time => "time",
            _ => "none",
        };

        /// <summary>What the engine said about one start.</summary>
        public static LookAheadAnswer Read(Wire.LookAheadAnswer answer)
        {
            return new LookAheadAnswer(
                answer.Start == null ? default : Read(answer.Start),
                (int)answer.Best,
                answer.Complete,
                (long)answer.ElapsedMs,
                (long)answer.DiagramNodes,
                (long)answer.NodesReached,
                StoppedByOf(answer.StoppedBy),
                BranchOf(answer.Branch),
                (int)answer.Destination);
        }

        /// <summary>What the engine said about a whole menu.</summary>
        public static LookAheadResponse Read(Wire.LookAheadResponse? response)
        {
            if (response == null)
            {
                return LookAheadResponse.Of(Array.Empty<LookAheadAnswer>(), null);
            }

            LookAheadAnswer[] answers = response.Answers.Select(Read).ToArray();

            // HasError rather than a null check, because the schema gives the member
            // explicit presence: an error that was genuinely the empty string is still an
            // error, and reading it off the value alone would turn it into a success.
            return LookAheadResponse.Of(answers, response.HasError ? response.Error : null);
        }

        /// <summary>Every question a crawl over one group can ask.</summary>
        public static LookAheadQuestions Read(Wire.Questions? questions)
        {
            questions ??= new Wire.Questions();
            return LookAheadQuestions.Of(
                questions.Conversations.ToArray(),
                questions.Variables.ToArray(),
                questions.Queries.ToArray(),
                questions.Items.ToArray(),
                questions.Thoughts.ToArray(),
                questions.Checks.Select(Read).ToArray(),
                questions.Entries.Select(Read).ToArray(),
                questions.Data
                    .Select(request => new DataRequest(Read(request.Kind), request.Subject))
                    .ToArray());
        }

        /// <summary>One data kind, as the engine's callers speak it.</summary>
        /// <remarks>
        /// Written out rather than cast, so a value added on one side fails to compile here
        /// rather than arriving as a number nothing handles. A kind this build does not know
        /// becomes <see cref="DataKind.Unspecified"/>, which is serviced by nobody and reads
        /// Unknown - the permissive answer, and the right one for a newer engine talking to
        /// an older plugin.
        /// </remarks>
        private static DataKind Read(Wire.DataKind kind)
        {
            switch (kind)
            {
                case Wire.DataKind.ThoughtsCooking:
                    return DataKind.ThoughtsCooking;
                case Wire.DataKind.ThoughtsFixed:
                    return DataKind.ThoughtsFixed;
                case Wire.DataKind.EquippedInSlot:
                    return DataKind.EquippedInSlot;
                case Wire.DataKind.TabHoldsItems:
                    return DataKind.TabHoldsItems;
                case Wire.DataKind.ItemsInGroup:
                    return DataKind.ItemsInGroup;
                case Wire.DataKind.HeldItemsInGroup:
                    return DataKind.HeldItemsInGroup;
                case Wire.DataKind.SceneIsOutside:
                    return DataKind.SceneIsOutside;
                case Wire.DataKind.SkillDamage:
                    return DataKind.SkillDamage;
                case Wire.DataKind.GameMode:
                    return DataKind.GameMode;
                case Wire.DataKind.HardcorePlaythroughCompleted:
                    return DataKind.HardcorePlaythroughCompleted;
                case Wire.DataKind.PartyFlag:
                    return DataKind.PartyFlag;
                default:
                    return DataKind.Unspecified;
            }
        }
    }
}
