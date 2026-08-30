// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Linq;
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class ActionParserTests
    {
        private const int CounterCap = 16;

        private static LookAheadState Run(
            string script, StateSymbols symbols, LookAheadState state, int onceSlot = 0)
        {
            IReadOnlyList<DialogueAction> actions = ActionParser.Parse(script, symbols);
            return DialogueAction.Apply(actions, state, onceSlot, CounterCap);
        }

        [Fact]
        public void SetVariableValue_AssignsTrue()
        {
            var symbols = new StateSymbols();
            int once = symbols.Once(GraphBuilder.Node(0));
            LookAheadState state = Run(
                "SetVariableValue(\"whirling.lena_intro_done\", true) --[[ Variable[ ]]",
                symbols,
                LookAheadState.Empty(symbols.Count, 0),
                once);

            Assert.True(state.IsSet(symbols.Variable("whirling.lena_intro_done")));
        }

        [Fact]
        public void MultipleStatements_AllApply()
        {
            var symbols = new StateSymbols();
            LookAheadState state = Run(
                "GainItem(\"shoes_faln\");\n"
                + "SetVariableValue(\"jam.siileng_bought_faln_sneakers\", true) --[[ Variable[ ]]",
                symbols,
                LookAheadState.Empty(symbols.Count, 0),
                symbols.Once(GraphBuilder.Node(0)));

            Assert.True(state.IsSet(symbols.Item("shoes_faln")));
            Assert.True(state.IsSet(symbols.Variable("jam.siileng_bought_faln_sneakers")));
        }

        [Fact]
        public void LoseItem_ClearsTheSlot()
        {
            var symbols = new StateSymbols();
            int slot = symbols.Item("commemorative_pin");
            LookAheadState start = LookAheadState.Empty(symbols.Count, 0).With(slot, 1);
            LookAheadState state = Run("LoseItem(\"commemorative_pin\")", symbols, start);

            Assert.False(state.IsSet(slot));
        }

        [Theory]
        [InlineData("GainTask(\"TASK.x\")", true)]
        [InlineData("FinishTask(\"TASK.x\")", false)]
        [InlineData("CancelTask(\"TASK.x\")", false)]
        public void TaskCalls_SetAndClear(string script, bool expected)
        {
            var symbols = new StateSymbols();
            int slot = symbols.Task("TASK.x");
            LookAheadState start = LookAheadState.Empty(symbols.Count, 0).With(slot, 1);
            LookAheadState state = Run(script, symbols, start);

            Assert.Equal(expected, state.IsSet(slot));
        }

        /// <summary>The counter idiom from conversation 825, verbatim.</summary>
        [Fact]
        public void OnceIncrement_AddsThenStops()
        {
            var symbols = new StateSymbols();
            int counter = symbols.Variable("whirling.lena_quiz_wrong_counter");
            int once = symbols.Once(GraphBuilder.Node(7));
            const string Script =
                "SetVariableValue(\"whirling.lena_quiz_wrong_counter\", "
                + "Variable[\"whirling.lena_quiz_wrong_counter\"] +once(2)) ";

            LookAheadState first = Run(Script, symbols, LookAheadState.Empty(symbols.Count, 0), once);
            Assert.Equal(2, first.Get(counter));

            // Reaching the same node again on the same path must not add again.
            LookAheadState second = Run(Script, symbols, first, once);
            Assert.Equal(2, second.Get(counter));
        }

        [Fact]
        public void Increment_SaturatesAtTheCap()
        {
            var symbols = new StateSymbols();
            int counter = symbols.Variable("q.count");
            LookAheadState state = LookAheadState.Empty(symbols.Count, 0).With(counter, CounterCap);
            IReadOnlyList<DialogueAction> actions = ActionParser.Parse(
                "SetVariableValue(\"q.count\", Variable[\"q.count\"] + 3)", symbols);

            LookAheadState raised = DialogueAction.Apply(actions, state, 0, CounterCap);
            Assert.Equal(CounterCap, raised.Get(counter));
        }

        [Theory]
        [InlineData("GainMoneyAlways(40)", 140)]
        [InlineData("LoseMoneyAlways(40)", 60)]
        public void MoneyAlways_MovesTheBalance(string script, int expected)
        {
            var symbols = new StateSymbols();
            LookAheadState state = Run(script, symbols, LookAheadState.Empty(symbols.Count, 100));
            Assert.Equal(expected, state.Money);
        }

        /// <summary>
        /// All 28 GainMoneyOnce nodes sit inside a cycle, so "once" is what stops the
        /// search minting money.
        /// </summary>
        [Fact]
        public void GainMoneyOnce_PaysOnlyTheFirstTime()
        {
            var symbols = new StateSymbols();
            int once = symbols.Once(GraphBuilder.Node(3));
            LookAheadState first = Run(
                "GainMoneyOnce(500)", symbols, LookAheadState.Empty(symbols.Count, 0), once);
            Assert.Equal(500, first.Money);

            LookAheadState second = Run("GainMoneyOnce(500)", symbols, first, once);
            Assert.Equal(500, second.Money);
        }

        [Fact]
        public void MoneyNeverGoesNegative()
        {
            var symbols = new StateSymbols();
            LookAheadState state = Run(
                "LoseMoneyAlways(500)", symbols, LookAheadState.Empty(symbols.Count, 100));
            Assert.Equal(0, state.Money);
        }

        /// <summary>
        /// Calls outside the model are kept rather than dropped, so a later pass can
        /// find them and a reader can see they were considered.
        /// </summary>
        [Fact]
        public void UnmodelledCalls_AreRecordedButChangeNothing()
        {
            var symbols = new StateSymbols();
            IReadOnlyList<DialogueAction> actions = ActionParser.Parse(
                "ReputationGrows(\"honour\");\nPassTime(2)", symbols);

            Assert.Equal(2, actions.Count);
            Assert.All(actions, a => Assert.Equal(DialogueActionKind.Unmodelled, a.Kind));
            Assert.Contains(actions, a => a.Name == "PassTime");

            LookAheadState before = LookAheadState.Empty(symbols.Count, 250);
            LookAheadState after = DialogueAction.Apply(actions, before, 0, CounterCap);
            Assert.Equal(250, after.Money);
        }

        [Fact]
        public void EmptyScript_ProducesNoActions()
        {
            var symbols = new StateSymbols();
            Assert.Empty(ActionParser.Parse(null, symbols));
            Assert.Empty(ActionParser.Parse("   ", symbols));
        }
    }
}
