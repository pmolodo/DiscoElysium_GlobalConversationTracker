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
    /// <param name="Uncertain">A search that gave up before it could say.</param>
    /// <param name="BranchUncertain">The same, on the branch line - see the remarks
    /// on <see cref="BranchLine"/> for why it is a different colour.</param>
    /// <param name="MarkUncertain">Whether a search that gave up says so at all.</param>
    public readonly record struct MarkerPalette(
        string UnseenAnyGame,
        string UnseenThisGame,
        string Seen,
        string Uncertain,
        string BranchUncertain,
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
        /// The line for one option, or null where the option is not a rolled check.
        /// </summary>
        /// <param name="answer">What the engine said about the option.</param>
        /// <param name="palette">The colours to paint it in.</param>
        /// <returns>
        /// Markup beginning with a newline, so a caller appends it to the option's own text;
        /// or null, which is what an option with one outcome gets.
        /// </returns>
        public static string? For(LookAheadAnswer answer, MarkerPalette palette)
        {
            if (answer.Branches is not BranchAnswers branches)
            {
                // Not a roll. The engine fills this only for white and red checks, so its
                // absence is the answer rather than a gap - see de-fes.1.
                return null;
            }

            var line = new StringBuilder("\n");
            line.Append(' ', Math.Max(0, (LineWidth / 3) - (PassWord.Length / 2)));
            line.Append(Half(PassWord, branches.Pass, palette));
            line.Append(' ', Math.Max(1, (LineWidth / 3) - PassWord.Length));
            line.Append(Half(FailWord, branches.Fail, palette));
            return line.ToString();
        }

        /// <summary>One half of the line: the word, in its colour, and its asterisk.</summary>
        /// <remarks>
        /// The asterisk follows the option rule exactly. Something BEYOND the destination
        /// outranks it, so there is more down there than the outcome itself shows; nothing
        /// does, so there is not - and a search that gave up says so rather than claiming
        /// the second, for the reason de-pvq gives.
        /// </remarks>
        private static string Half(string word, BranchAnswer branch, MarkerPalette palette)
        {
            string coloured = Draw(ColourOf((Novelty)branch.Destination, palette), word);

            if (branch.Best > branch.Destination)
            {
                string html = branch.Best == (int)Novelty.UnseenAnyGame
                    ? palette.UnseenAnyGame
                    : palette.UnseenThisGame;
                return coloured + Draw(html, FoundMarker);
            }

            // THE BRANCH LINE'S OWN UNCERTAIN COLOUR, not the option's. An option is
            // drawn on black, where the mid grey the mod uses reads perfectly well; this
            // line is drawn on the check's own background, where the same grey measured
            // 1.08:1 and simply could not be seen. See de-8hh2.4.
            return palette.MarkUncertain && !branch.Complete
                ? coloured + Draw(palette.BranchUncertain, UncertainMarker)
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
