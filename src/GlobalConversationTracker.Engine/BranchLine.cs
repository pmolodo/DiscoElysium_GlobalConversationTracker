// SPDX-License-Identifier: MIT
using System;
using System.Text;

namespace GlobalConversationTracker.Engine
{
    /// <summary>The colours the mod paints look-ahead information in.</summary>
    /// <param name="UnseenAnyGame">Reaching text no save has read.</param>
    /// <param name="UnseenThisGame">Reaching text this save has not read.</param>
    /// <param name="Seen">Already read. Used on the branch line only - see the remarks
    /// on <see cref="BranchLine"/>.</param>
    /// <param name="Uncertain">A search that gave up before it could say. One colour for
    /// the option and for the branch line alike: both are drawn on black, since the check's
    /// band is shortened to make room for the line under it.</param>
    /// <param name="MarkUncertain">Whether a search that gave up says so at all.</param>
    public readonly record struct MarkerPalette(
        string UnseenAnyGame,
        string UnseenThisGame,
        string Seen,
        string Uncertain,
        bool MarkUncertain);

    /// <summary>
    /// The Pass / Fail line drawn below a white or red check option.
    /// </summary>
    /// <remarks>
    /// <para>WHY IT EXISTS. A white or red check is TWO options wearing one line of text.
    /// Until this line, the mod reported the better of the two outcomes and left the player
    /// unable to tell which: an option where passing led nowhere new and failing led
    /// somewhere new looked exactly like its opposite, and both looked like one where
    /// either did.</para>
    ///
    /// <para>IT USES THE ESTABLISHED CONVENTIONS RATHER THAN INVENTING ANY. Each word's
    /// colour is that outcome's own seen status - the same three the mod paints an option
    /// in - and the asterisk after it is that outcome's reachable status, by the same rule
    /// an option's own marker follows. A player who has learnt the colours on an option
    /// already knows them here; the only new thing to learn is that the two words name the
    /// two outcomes.</para>
    ///
    /// <para>WHY A SEPARATE LINE AND NOT THE OPTION'S OWN. A check is drawn on a red or
    /// white background with its own text colour to suit, and the mod's three colours are
    /// legible because of the black behind them. On the check's own background they stop
    /// being readable, and a second scheme used only on checks would mean a player learning
    /// that orange means one thing on an option and another on a check - at which point the
    /// convention has stopped being one. The line costs a line of vertical space per check
    /// and buys an unambiguous reading. That is the trade, made deliberately.</para>
    ///
    /// <para>Here rather than in the plugin because it is pure text: an answer and a
    /// palette in, markup out. The plugin assembly cannot be unit-tested - it is built
    /// against the game's IL2CPP types - and this is the half worth testing.</para>
    /// </remarks>
    public static class BranchLine
    {
        /// <summary>The word naming the outcome where the check succeeds.</summary>
        public const string PassWord = "Pass";

        /// <summary>The word naming the outcome where it fails.</summary>
        public const string FailWord = "Fail";

        /// <summary>The marker for an outcome that reaches something.</summary>
        public const string FoundMarker = "*";

        /// <summary>The marker for an outcome whose search gave up.</summary>
        public const string UncertainMarker = "*?";

        /// <summary>
        /// How wide the line is taken to be, in the spaces it is padded with.
        /// </summary>
        /// <remarks>
        /// THE FONT IS NOT MONOSPACED, so this centres nothing exactly and is not trying
        /// to. The design asks for the two words roughly a third and two thirds across;
        /// padding to a fixed column count puts them near enough that they read as two
        /// columns, which is the requirement. Measuring the text window instead would put
        /// the layout at the mercy of a font size the player can change.
        /// </remarks>
        public const int LineWidth = 48;

        /// <summary>
        /// The line for one rolled check, from the two answers its outcomes came back as.
        /// </summary>
        /// <remarks>
        /// TWO ANSWERS RATHER THAN ONE WITH A PAIR INSIDE IT, since de-8hh2.6. A check is
        /// two options wearing one line of text, and the engine now says so directly:
        /// asking about it returns one answer per outcome, each an ordinary answer with its
        /// own baseline and its own cost. The caller finds the pair - see
        /// <see cref="LookAheadResponse.OutcomesOf"/> - and there being no pair is what says
        /// the option is not a roll, which is the check the caller makes instead of this
        /// one.
        /// </remarks>
        /// <param name="pass">What the engine said about the outcome where the check succeeds.</param>
        /// <param name="fail">And about the one where it fails.</param>
        /// <param name="palette">The colours to paint it in.</param>
        /// <returns>
        /// Markup beginning with a newline, so a caller appends it to the option's own text.
        /// </returns>
        public static string For(
            LookAheadAnswer pass, LookAheadAnswer fail, MarkerPalette palette)
        {
            var line = new StringBuilder("\n");
            line.Append(' ', Math.Max(0, (LineWidth / 3) - (PassWord.Length / 2)));
            line.Append(Half(PassWord, pass, palette));
            line.Append(' ', Math.Max(1, (LineWidth / 3) - PassWord.Length));
            line.Append(Half(FailWord, fail, palette));
            return line.ToString();
        }

        /// <summary>One half of the line: the word, in its colour, and its asterisk.</summary>
        /// <remarks>
        /// The asterisk follows the option rule exactly. Something BEYOND the destination
        /// outranks it, so there is more down there than the outcome itself shows; nothing
        /// does, so there is not - and a search that gave up says so rather than claiming
        /// the second, for the reason de-pvq gives.
        /// </remarks>
        private static string Half(string word, LookAheadAnswer branch, MarkerPalette palette)
        {
            string coloured = Draw(ColourOf((Novelty)branch.Destination, palette), word);

            if (branch.Best > branch.Destination)
            {
                string html = branch.Best == (int)Novelty.UnseenAnyGame
                    ? palette.UnseenAnyGame
                    : palette.UnseenThisGame;
                return coloured + Draw(html, FoundMarker);
            }

            // AN OUTCOME ON THE TOP RUNG IS NOT UNCERTAIN ABOUT ANYTHING. The asterisk
            // answers "does something beyond this outrank it", and where the outcome
            // already lands on text no save has read, nothing can - so the answer is no,
            // settled, whether or not the search finished. Drawing '*?' there would claim
            // a doubt about a question that has none. The option's own marker has always
            // reasoned this way, refusing a search it can prove pointless before it starts.
            if (branch.Destination >= (int)Novelty.UnseenAnyGame)
            {
                return coloured;
            }

            // THE OPTION'S OWN UNCERTAIN COLOUR, which is the only one there is. This line
            // used to need a second, near-white, because it was drawn on the check's own
            // background - the option's mid grey measured 1.08:1 against a white check's
            // band and could not be seen at all (de-8hh2.4). CheckBandInsetPatch shortens
            // that band so the line falls on black instead, which is what the grey was
            // chosen against, so the second colour went with the background that needed it.
            return palette.MarkUncertain && !branch.Complete
                ? coloured + Draw(palette.Uncertain, UncertainMarker)
                : coloured;
        }

        /// <summary>The colour the mod paints one novelty in.</summary>
        private static string ColourOf(Novelty novelty, MarkerPalette palette) => novelty switch
        {
            Novelty.UnseenAnyGame => palette.UnseenAnyGame,
            Novelty.UnseenThisGame => palette.UnseenThisGame,
            _ => palette.Seen,
        };

        /// <summary>One run of text, in one colour, as the game's markup.</summary>
        private static string Draw(string colourHtml, string text) =>
            "<color=" + colourHtml + ">" + text + "</color>";
    }
}
