// SPDX-License-Identifier: MIT
using System;
using DiscoPages.Elements.Dialogue;
using HarmonyLib;

namespace GlobalConversationTracker.TestProbe
{
    /// <summary>
    /// Reads what the dialogue interface is waiting for: a continue, or a choice.
    /// </summary>
    /// <remarks>
    /// <para>WHY A RUN WANTS THIS. The harness advances a conversation to its menu by
    /// answering each reported line with one continue, and it decides a line was the last
    /// one by WAITING three seconds to see whether a menu follows. That is a probability,
    /// not an answer - the longer the wait, the likelier it was right - and it costs about
    /// forty seconds a run. A state the interface can be asked for settles it outright.
    /// </para>
    ///
    /// <para>WHAT HAS ALREADY BEEN RULED OUT, at three in-game runs. The class that looks
    /// like the answer, <c>ContinueResponseTogglePageSystem</c>, is never instantiated in
    /// this build: seven hooks over every member of it, <c>Update</c> included, applied
    /// cleanly per the Harmony log and not one ever fired.
    /// <c>UnityEngine.Object.FindObjectOfType&lt;T&gt;()</c> does not resolve an IL2CPP
    /// type through the interop's generic overload either.</para>
    ///
    /// <para>So this reads the two remaining candidates AT ONCE rather than one per run:
    /// the continue button, which carries a <c>ContState</c> and a static instance that
    /// needs no hook at all, and the mouse UI's <c>ContinueResponseToggle</c>, which is
    /// the sibling of the dead class. A run then says which of them is live and what each
    /// says while a line is up and while a menu is.</para>
    /// </remarks>
    internal static class DialogueWaitProbe
    {
        /// <summary>Installs the transition hooks.</summary>
        /// <param name="harmony">The probe's patcher.</param>
        internal static void Install(Harmony harmony)
        {
            harmony.PatchAll(typeof(ContinueButtonStateProbe));
            harmony.PatchAll(typeof(ToggleChooseProbe));
            harmony.PatchAll(typeof(ToggleUpdateProbe));
        }

        /// <summary>What the dialogue is waiting for.</summary>
        internal enum Waiting
        {
            /// <summary>Neither yet: a sequence playing, a menu being built.</summary>
            Nothing,

            /// <summary>A line is up and the continue is offered.</summary>
            Continue,

            /// <summary>A response menu is up. Nothing may be advanced.</summary>
            Options,

            /// <summary>
            /// The only thing on offer closes the conversation, so there is no menu coming.
            /// </summary>
            Ending,
        }

        /// <summary>
        /// What the interface is waiting for, from the two things that answer.
        /// </summary>
        /// <remarks>
        /// <para>MEASURED IN GAME, 2026-09-04, over a run of the money and pristine suites.
        /// At every completed menu, eight for eight, the toggle read OPTIONS and the button
        /// read DISABLED; at a line waiting to be advanced the button read ENABLED and the
        /// toggle NONE; before either, both were quiet. So the toggle is the menu's
        /// signal and the button is the line's.</para>
        ///
        /// <para>THE TOGGLE IS ASKED FIRST, and answers for both. A menu is the state
        /// nothing may be advanced from, so where the two disagree the toggle wins - and
        /// they can disagree for a frame while the interface changes over.</para>
        ///
        /// <para>WHAT THIS STILL CANNOT SAY is that a menu is COMING. The last line before
        /// a menu reads exactly like any other line - 451:63 read ENABLED at the instant it
        /// went up, and the menu arrived beside it a few frames later. So a caller still
        /// has to let a line settle before answering it; what this buys is that the settling
        /// happens inside the game against a state that changes on the frame it changes,
        /// rather than outside it against a log read twice a second.</para>
        /// </remarks>
        internal static Waiting WhatIsWaiting()
        {
            // THE MENU IS THE PROBE'S OWN FACT, not the toggle's. The toggle keeps the
            // last state it was put in, so it still reads OPTIONS from the PREVIOUS
            // scenario's menu when a new conversation opens - measured: the first line of
            // conversation 28 read OPTIONS before anything in it had been drawn. Counting
            // the menus this conversation has actually composed cannot go stale, because
            // starting a conversation resets it.
            if (TestProbePlugin.MenusShown > 0)
            {
                return Waiting.Options;
            }

            return Offering(ContinueButton());
        }

        /// <summary>What a continue button in this state is offering.</summary>
        /// <remarks>
        /// EVERY FLAVOUR OF CONTINUE COUNTS, not just ENABLED. The game colours the button
        /// by the skill that is speaking - INT, PSY, FYS, MOT - and HAZY is a continue too.
        /// Reading only ENABLED cost a run: Garte's line offered PSY, the loop called that
        /// "nothing is waiting", and it sat there until it gave up.
        ///
        /// ENDING IS NOT CONTINUING. On END_CONVERSATION the button closes the
        /// conversation rather than advancing it, so pressing it would leave - which is
        /// never what a run reaching for a menu wants, and is worth reporting as its own
        /// answer rather than as a line that would not advance.
        /// </remarks>
        private static Waiting Offering(string state) => state switch
        {
            "DISABLED" => Waiting.Nothing,
            "END_CONVERSATION" or "END_CONVERSATION_SPECIAL" => Waiting.Ending,
            "no-instance" => Waiting.Nothing,
            _ when state.StartsWith("unreadable", StringComparison.Ordinal) => Waiting.Nothing,
            _ => Waiting.Continue,
        };

        /// <summary>
        /// What the continue button says, or why it cannot be asked.
        /// </summary>
        /// <remarks>
        /// A PLAIN STATIC FIELD is the whole reason this one is worth trying:
        /// <c>SunshineContinueButton.instance</c> needs no hook, no lookup and no
        /// singleton accessor on a generic base class - the three things that made the
        /// last attempt fail.
        /// </remarks>
        internal static string ContinueButton()
        {
            try
            {
                SunshineContinueButton button = SunshineContinueButton.instance;
                return button == null ? "no-instance" : button.State.ToString();
            }
            catch (Exception error)
            {
                return "unreadable:" + error.GetType().Name;
            }
        }

        /// <summary>What the mouse UI's toggle says, or why it cannot be asked.</summary>
        internal static string Toggle()
        {
            try
            {
                ContinueResponseToggle? toggle = _toggle;
                return toggle == null ? "no-instance" : toggle.State.ToString();
            }
            catch (Exception error)
            {
                return "unreadable:" + error.GetType().Name;
            }
        }

        private static ContinueResponseToggle? _toggle;

        /// <summary>The continue button changing state, which is the transition itself.</summary>
        [HarmonyPatch(typeof(SunshineContinueButton), nameof(SunshineContinueButton.SetState))]
        private static class ContinueButtonStateProbe
        {
            /// <summary>The parameter name has to stay <c>buttonState</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(ContState buttonState)
            {
                ProbeLog.Write("continue-button", "state", buttonState.ToString());
            }
        }

        /// <summary>The mouse toggle deciding what to show.</summary>
        [HarmonyPatch(
            typeof(ContinueResponseToggle),
            nameof(ContinueResponseToggle.ChooseState))]
        private static class ToggleChooseProbe
        {
            /// <summary>The parameter name has to stay <c>__instance</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(ContinueResponseToggle __instance)
            {
                _toggle = __instance;
                ProbeLog.Write("toggle-chose", "state", Toggle());
            }
        }

        /// <summary>
        /// The mouse toggle's per-frame tick, reported only when its state changes.
        /// </summary>
        /// <remarks>
        /// The one that says whether this class is alive at all. If it never fires, the
        /// mouse toggle is as dead as its page-system sibling and the continue button is
        /// the only candidate left.
        /// </remarks>
        [HarmonyPatch(typeof(ContinueResponseToggle), "Update")]
        private static class ToggleUpdateProbe
        {
            private static string _last = string.Empty;

            /// <summary>The parameter name has to stay <c>__instance</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(ContinueResponseToggle __instance)
            {
                try
                {
                    _toggle = __instance;
                    string now = Toggle();
                    if (_last == now)
                    {
                        return;
                    }

                    _last = now;
                    ProbeLog.Write("toggle-state", "state", now);
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("reading the dialogue toggle", error);
                }
            }
        }
    }
}
