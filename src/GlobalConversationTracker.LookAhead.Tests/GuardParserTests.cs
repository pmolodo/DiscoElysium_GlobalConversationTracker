// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class GuardParserTests
    {
        private static Ternary Test(string? guard, FakeWorld world)
        {
            return GuardParser.Parse(guard).Test(new WorldContext(world));
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        public void NoCondition_IsTrue(string? guard)
        {
            Assert.Equal(Ternary.True, Test(guard, new FakeWorld()));
        }

        [Fact]
        public void BareVariable_ReadsAsTruthiness()
        {
            var world = new FakeWorld().WithVariable("a.b", true);
            Assert.Equal(Ternary.True, Test("Variable[\"a.b\"]", world));
        }

        /// <summary>
        /// The exporter's negation form, and by a wide margin the most common negative
        /// shape in the database: 5,618 guards are exactly <c>(V) == false</c>.
        /// </summary>
        [Theory]
        [InlineData(true, Ternary.False)]
        [InlineData(false, Ternary.True)]
        public void ParenthesisedEqualsFalse_IsNegation(bool value, Ternary expected)
        {
            var world = new FakeWorld().WithVariable("a.b", value);
            Assert.Equal(expected, Test("(Variable[\"a.b\"]) == false", world));
        }

        [Fact]
        public void BlockComments_AreStripped()
        {
            var world = new FakeWorld().WithQuery("IsTaskActive", true);
            Assert.Equal(
                Ternary.True,
                Test("IsTaskActive(\"TASK.x\")--[[ Variable[ ]]", world));
        }

        [Fact]
        public void UnknownQuery_IsUnknownNotFalse()
        {
            Assert.Equal(Ternary.Unknown, Test("IsKimHere()", new FakeWorld()));
        }

        /// <summary>False beats Unknown: one definitely-false conjunct settles it.</summary>
        [Fact]
        public void AndWithDefiniteFalse_IsFalseEvenWhenTheOtherIsUnknown()
        {
            var world = new FakeWorld().WithVariable("a.b", false);
            Assert.Equal(Ternary.False, Test("IsKimHere() and Variable[\"a.b\"]", world));
        }

        [Fact]
        public void OrWithDefiniteTrue_IsTrueEvenWhenTheOtherIsUnknown()
        {
            var world = new FakeWorld().WithVariable("a.b", true);
            Assert.Equal(Ternary.True, Test("IsKimHere() or Variable[\"a.b\"]", world));
        }

        [Theory]
        [InlineData(3, Ternary.True)]
        [InlineData(4, Ternary.False)]
        public void NumericComparison(double counter, Ternary expected)
        {
            var world = new FakeWorld().WithVariable("q.count", counter);
            Assert.Equal(expected, Test("Variable[\"q.count\"] < 4", world));
        }

        /// <summary>The negated counter guard that sits beside it in conversation 825.</summary>
        [Theory]
        [InlineData(3, Ternary.False)]
        [InlineData(4, Ternary.True)]
        public void NegatedNumericComparison(double counter, Ternary expected)
        {
            var world = new FakeWorld().WithVariable("q.count", counter);
            Assert.Equal(expected, Test("(Variable[\"q.count\"] < 4) == false", world));
        }

        /// <summary>
        /// Lua's <c>==</c> does not coerce across types, so a numeric variable is not
        /// equal to <c>true</c> however non-zero it is.
        /// </summary>
        [Fact]
        public void EqualityDoesNotCoerceAcrossTypes()
        {
            var world = new FakeWorld().WithVariable("q.count", 1d);
            Assert.Equal(Ternary.False, Test("Variable[\"q.count\"] == true", world));
        }

        /// <summary>A real multi-clause guard, copied from conversation 451.</summary>
        [Fact]
        public void SiilengSpeakersGuard_ParsesAndEvaluates()
        {
            const string Guard =
                "Variable[\"jam.siileng_bought_faln_sneakers\"] == true"
                + "  and  Variable[\"jam.siileng_learned_when_you_can_buy_speakers\"] == true"
                + "  and  CheckItem(\"samaran_speakers\") == false";

            var ready = new FakeWorld()
                .WithVariable("jam.siileng_bought_faln_sneakers", true)
                .WithVariable("jam.siileng_learned_when_you_can_buy_speakers", true)
                .WithQuery("CheckItem", false);
            Assert.Equal(Ternary.True, Test(Guard, ready));

            var noSneakers = new FakeWorld()
                .WithVariable("jam.siileng_bought_faln_sneakers", false)
                .WithVariable("jam.siileng_learned_when_you_can_buy_speakers", true)
                .WithQuery("CheckItem", false);
            Assert.Equal(Ternary.False, Test(Guard, noSneakers));
        }

        [Fact]
        public void OperatorPrecedence_AndBindsTighterThanOr()
        {
            // false and false or true  ==  (false and false) or true  ==  true
            var world = new FakeWorld()
                .WithVariable("a", false)
                .WithVariable("b", false)
                .WithVariable("c", true);
            Assert.Equal(
                Ternary.True,
                Test("Variable[\"a\"] and Variable[\"b\"] or Variable[\"c\"]", world));
        }

        [Fact]
        public void Garbage_Throws()
        {
            Assert.Throws<GuardParseException>(() => GuardParser.Parse("Variable[\"a\"] $$ 3"));
        }

        [Fact]
        public void TryParse_ReportsFailureWithoutThrowing()
        {
            Assert.False(GuardParser.TryParse("Variable[\"a\"] $$ 3", out GuardExpression fallback));
            Assert.Equal(Ternary.True, fallback.Test(new WorldContext(new FakeWorld())));
        }

        /// <summary>Adapts a world to the guard-evaluation interface, for parser tests.</summary>
        private sealed class WorldContext : IGuardContext
        {
            private readonly FakeWorld _world;

            public WorldContext(FakeWorld world)
            {
                _world = world;
            }

            public GuardValue GetVariable(string name)
            {
                return _world.GetVariable(name);
            }

            public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
            {
                return _world.Query(name, arguments);
            }
        }
    }
}
