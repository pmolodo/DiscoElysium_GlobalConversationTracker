// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Automation
{
    /// <summary>One run of consecutive frames showing the same screen.</summary>
    public sealed class TimelineStage
    {
        /// <summary>Creates a stage.</summary>
        /// <param name="firstFrame">Index of the first frame, 0-based.</param>
        /// <param name="lastFrame">Index of the last frame, 0-based.</param>
        /// <param name="meanDetail">Mean detail across the stage.</param>
        public TimelineStage(int firstFrame, int lastFrame, double meanDetail)
        {
            FirstFrame = firstFrame;
            LastFrame = lastFrame;
            MeanDetail = meanDetail;
        }

        /// <summary>Index of the first frame, 0-based.</summary>
        public int FirstFrame { get; }

        /// <summary>Index of the last frame, 0-based.</summary>
        public int LastFrame { get; }

        /// <summary>How many frames the stage lasted.</summary>
        public int Length => LastFrame - FirstFrame + 1;

        /// <summary>Mean detail across the stage; near zero means a nearly blank screen.</summary>
        public double MeanDetail { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"frames {FirstFrame + 1}-{LastFrame + 1} ({Length}), detail {MeanDetail:N3}";
        }
    }

    /// <summary>How well one frame identifies its own stage against all the others.</summary>
    public sealed class ReferenceQuality
    {
        /// <summary>Creates a result.</summary>
        /// <param name="frame">The candidate frame, 0-based.</param>
        /// <param name="worstWithinStage">Largest difference to a frame of the same stage.</param>
        /// <param name="bestOutsideStage">Smallest difference to a frame of another stage.</param>
        public ReferenceQuality(int frame, double worstWithinStage, double bestOutsideStage)
        {
            Frame = frame;
            WorstWithinStage = worstWithinStage;
            BestOutsideStage = bestOutsideStage;
        }

        /// <summary>The candidate frame, 0-based.</summary>
        public int Frame { get; }

        /// <summary>
        /// The largest difference to another frame of the SAME stage. A threshold must be
        /// above this, or the stage stops matching itself as it animates.
        /// </summary>
        public double WorstWithinStage { get; }

        /// <summary>
        /// The smallest difference to a frame of a DIFFERENT stage. A threshold must be
        /// below this, or another screen matches too.
        /// </summary>
        public double BestOutsideStage { get; }

        /// <summary>The gap a threshold has to live in. Negative means there is none.</summary>
        public double Margin => BestOutsideStage - WorstWithinStage;

        /// <summary>Whether any threshold separates this stage from the others.</summary>
        public bool IsUsable => Margin > 0;

        /// <summary>
        /// A threshold halfway across the gap, which is the most forgiving choice in both
        /// directions.
        /// </summary>
        public double SuggestedThreshold => WorstWithinStage + (Margin / 2);

        /// <inheritdoc/>
        public override string ToString()
        {
            return IsUsable
                ? $"frame {Frame + 1}: matches its own stage within {WorstWithinStage:N4}, "
                    + $"nearest other stage {BestOutsideStage:N4}, threshold {SuggestedThreshold:N4}"
                : $"frame {Frame + 1}: UNUSABLE - its own stage varies by {WorstWithinStage:N4} "
                    + $"but another stage is only {BestOutsideStage:N4} away";
        }
    }

    /// <summary>
    /// Finding the distinct screens in a recorded startup, and a frame that identifies
    /// each one.
    /// </summary>
    /// <remarks>
    /// <para>Startup is a sequence of screens, so "has it finished loading" is really "is
    /// this the last screen". Answering it needs a reference image and a threshold, and
    /// both should come from measurement: the game's menu animates, so a reference does
    /// not match itself exactly, while the legal notice holds still for twenty seconds and
    /// would satisfy any test for stillness.</para>
    ///
    /// <para>A threshold has to sit above how much a screen varies within itself and below
    /// how far apart two screens are. This measures both, and says so when no such gap
    /// exists rather than picking a number that looks reasonable.</para>
    /// </remarks>
    public static class TimelineAnalysis
    {
        /// <summary>How much difference between consecutive frames counts as a new screen.</summary>
        /// <remarks>
        /// Measured, not chosen: within a stage consecutive frames differ by under 0.05
        /// even on the animated menu, while a stage change is 0.08 upwards and usually far
        /// more - 0.44 and 0.73 have both been recorded at real boundaries.
        /// </remarks>
        public const double DefaultBoundary = 0.06;

        /// <summary>Splits a recorded run into stages.</summary>
        /// <param name="fingerprints">One fingerprint per frame, in order.</param>
        /// <param name="boundary">Difference between frames that starts a new stage.</param>
        /// <exception cref="ArgumentNullException"><paramref name="fingerprints"/> is null.</exception>
        public static TimelineStage[] FindStages(
            IReadOnlyList<double[]> fingerprints, double boundary = DefaultBoundary)
        {
            if (fingerprints == null)
            {
                throw new ArgumentNullException(nameof(fingerprints));
            }

            var stages = new List<TimelineStage>();
            if (fingerprints.Count == 0)
            {
                return stages.ToArray();
            }

            int start = 0;
            for (int i = 1; i <= fingerprints.Count; i++)
            {
                bool ends = i == fingerprints.Count
                    || GameScreen.Difference(fingerprints[i - 1], fingerprints[i]) > boundary;

                if (ends)
                {
                    double detail = 0;
                    for (int j = start; j < i; j++)
                    {
                        detail += GameScreen.Detail(fingerprints[j]);
                    }

                    stages.Add(new TimelineStage(start, i - 1, detail / (i - start)));
                    start = i;
                }
            }

            return stages.ToArray();
        }

        /// <summary>Measures how well a frame identifies its own stage.</summary>
        /// <param name="fingerprints">One fingerprint per frame, in order.</param>
        /// <param name="stage">The stage the candidate belongs to.</param>
        /// <param name="frame">The candidate frame, 0-based.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException">The frame is not in the stage.</exception>
        public static ReferenceQuality Measure(
            IReadOnlyList<double[]> fingerprints, TimelineStage stage, int frame)
        {
            if (fingerprints == null)
            {
                throw new ArgumentNullException(nameof(fingerprints));
            }

            if (stage == null)
            {
                throw new ArgumentNullException(nameof(stage));
            }

            if (frame < stage.FirstFrame || frame > stage.LastFrame)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(frame), $"Frame {frame} is not inside {stage}.");
            }

            double worstWithin = 0;
            double bestOutside = double.MaxValue;

            for (int i = 0; i < fingerprints.Count; i++)
            {
                if (i == frame)
                {
                    continue;
                }

                double difference = GameScreen.Difference(fingerprints[frame], fingerprints[i]);
                bool sameStage = i >= stage.FirstFrame && i <= stage.LastFrame;

                if (sameStage)
                {
                    if (difference > worstWithin)
                    {
                        worstWithin = difference;
                    }
                }
                else if (difference < bestOutside)
                {
                    bestOutside = difference;
                }
            }

            return new ReferenceQuality(
                frame, worstWithin, bestOutside == double.MaxValue ? 1.0 : bestOutside);
        }

        /// <summary>Picks the frame of a stage that separates it best from the rest.</summary>
        /// <param name="fingerprints">One fingerprint per frame, in order.</param>
        /// <param name="stage">The stage to choose a reference from.</param>
        public static ReferenceQuality BestReference(
            IReadOnlyList<double[]> fingerprints, TimelineStage stage)
        {
            if (stage == null)
            {
                throw new ArgumentNullException(nameof(stage));
            }

            ReferenceQuality? best = null;
            for (int frame = stage.FirstFrame; frame <= stage.LastFrame; frame++)
            {
                ReferenceQuality candidate = Measure(fingerprints, stage, frame);
                if (best == null || candidate.Margin > best.Margin)
                {
                    best = candidate;
                }
            }

            return best!;
        }
    }
}
