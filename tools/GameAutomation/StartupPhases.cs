// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Automation
{
    /// <summary>One screen the game shows on its way to the main menu.</summary>
    public sealed class StartupPhase
    {
        /// <summary>Creates a phase.</summary>
        /// <param name="name">What to call it.</param>
        /// <param name="fingerprint">What it looks like, over <paramref name="region"/>.</param>
        /// <param name="threshold">How close a screen must be to count as this phase.</param>
        /// <param name="region">The part of the screen to compare, or null for all of it.</param>
        public StartupPhase(
            string name, double[] fingerprint, double threshold, Rectangle? region = null)
        {
            Name = name ?? throw new ArgumentNullException(nameof(name));
            Fingerprint = fingerprint ?? throw new ArgumentNullException(nameof(fingerprint));
            Threshold = threshold;
            Region = region;
        }

        /// <summary>What to call it.</summary>
        public string Name { get; }

        /// <summary>What it looks like, over <see cref="Region"/>.</summary>
        public double[] Fingerprint { get; }

        /// <summary>How close a screen must be to count as this phase.</summary>
        public double Threshold { get; }

        /// <summary>
        /// The part of the screen this phase is recognised by, or null for all of it.
        /// </summary>
        /// <remarks>
        /// For a screen that is partly animated. The logo screen is a still grey field
        /// with a logo animating in the middle of it, so it is recognised by the field and
        /// the middle is left out.
        ///
        /// Robustness rather than necessity: that animation is small against a whole frame
        /// and the logo stage was measured varying by only 0.0075 within itself, well
        /// under its threshold either way. Excluding it removes the one thing on that
        /// screen that moves, so the match stays tight if the animation ever changes.
        /// </remarks>
        public Rectangle? Region { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return Region == null
                ? $"{Name} (within {Threshold:N4})"
                : $"{Name} (within {Threshold:N4}, on {Region.Value.Width}x{Region.Value.Height} "
                    + $"at {Region.Value.X},{Region.Value.Y})";
        }
    }

    /// <summary>
    /// Knowing which startup screen is on display.
    /// </summary>
    /// <remarks>
    /// <para>Startup runs through several screens before the main menu, and one of them -
    /// the logo - can be skipped with a keypress. Knowing which is showing is what makes
    /// that possible, and it turns a silent fifty second wait into something that says
    /// where it has got to.</para>
    ///
    /// <para>Phases are stored as FINGERPRINTS, not images. The screenshots they came from
    /// are 1.5 to 2.3MB each and nothing ever looks at them again; the 64x64 greyscale
    /// fingerprint is all a comparison needs, and it is small enough to keep in the
    /// repository as text that diffs.</para>
    ///
    /// <para>Identification is deliberately allowed to fail. A screen matching nothing is
    /// reported as unknown rather than forced into the closest phase, because a wrong
    /// phase would have the harness pressing keys at the wrong screen. The caller falls
    /// back to simply waiting for the menu.</para>
    /// </remarks>
    public static class StartupPhases
    {
        /// <summary>The file phases are kept in, relative to the repository.</summary>
        public const string DefaultFileName = "startup-phases.txt";

        /// <summary>Reads phases from a file.</summary>
        /// <param name="path">The file to read.</param>
        /// <exception cref="FileNotFoundException">There is no such file.</exception>
        /// <exception cref="InvalidDataException">A line is malformed.</exception>
        public static StartupPhase[] Load(string path)
        {
            if (!File.Exists(path))
            {
                throw new FileNotFoundException($"No startup phases at {path}.", path);
            }

            var phases = new List<StartupPhase>();
            int number = 0;
            foreach (string raw in File.ReadAllLines(path))
            {
                number++;
                string line = raw.Trim();
                if (line.Length == 0 || line.StartsWith("#", StringComparison.Ordinal))
                {
                    continue;
                }

                string[] parts = line.Split('\t');
                if (parts.Length != 4)
                {
                    throw new InvalidDataException(
                        $"{path} line {number}: expected name, threshold, region and "
                        + $"fingerprint separated by tabs, got {parts.Length} field(s).");
                }

                if (!double.TryParse(
                    parts[1], NumberStyles.Float, CultureInfo.InvariantCulture, out double threshold))
                {
                    throw new InvalidDataException(
                        $"{path} line {number}: '{parts[1]}' is not a threshold.");
                }

                Rectangle? region = null;
                if (parts[2] != "-")
                {
                    string[] box = parts[2].Split(',');
                    if (box.Length != 4)
                    {
                        throw new InvalidDataException(
                            $"{path} line {number}: region wants x,y,width,height or '-'.");
                    }

                    region = new Rectangle(
                        int.Parse(box[0], CultureInfo.InvariantCulture),
                        int.Parse(box[1], CultureInfo.InvariantCulture),
                        int.Parse(box[2], CultureInfo.InvariantCulture),
                        int.Parse(box[3], CultureInfo.InvariantCulture));
                }

                string[] values = parts[3].Split(',');
                var fingerprint = new double[values.Length];
                for (int i = 0; i < values.Length; i++)
                {
                    if (!double.TryParse(
                        values[i], NumberStyles.Float, CultureInfo.InvariantCulture,
                        out fingerprint[i]))
                    {
                        throw new InvalidDataException(
                            $"{path} line {number}: value {i + 1} is not a number.");
                    }
                }

                phases.Add(new StartupPhase(parts[0], fingerprint, threshold, region));
            }

            return phases.ToArray();
        }

        /// <summary>Writes phases to a file.</summary>
        /// <param name="path">Where to write.</param>
        /// <param name="phases">What to write.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static void Save(string path, IEnumerable<StartupPhase> phases)
        {
            if (path == null)
            {
                throw new ArgumentNullException(nameof(path));
            }

            if (phases == null)
            {
                throw new ArgumentNullException(nameof(phases));
            }

            var text = new StringBuilder();
            text.AppendLine(
                "# Startup screens: name, threshold, region (x,y,w,h or -), fingerprint.");

            foreach (StartupPhase phase in phases)
            {
                text.Append(phase.Name);
                text.Append('\t');
                text.Append(phase.Threshold.ToString("N4", CultureInfo.InvariantCulture));
                text.Append('\t');
                text.Append(phase.Region == null
                    ? "-"
                    : $"{phase.Region.Value.X},{phase.Region.Value.Y},"
                        + $"{phase.Region.Value.Width},{phase.Region.Value.Height}");
                text.Append('\t');

                for (int i = 0; i < phase.Fingerprint.Length; i++)
                {
                    if (i > 0)
                    {
                        text.Append(',');
                    }

                    text.Append(phase.Fingerprint[i].ToString("N4", CultureInfo.InvariantCulture));
                }

                text.AppendLine();
            }

            string? directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory!);
            }

            File.WriteAllText(path, text.ToString());
        }

        /// <summary>Which phase a screen is, if any.</summary>
        /// <remarks>
        /// The closest phase within its own threshold. Nothing close enough is null - an
        /// unrecognised screen, which the caller should treat as "keep waiting" rather
        /// than guess at.
        /// </remarks>
        /// <param name="screen">The screen to identify.</param>
        /// <param name="phases">The phases to match against.</param>
        /// <param name="difference">How far the answer was, or 1 for no match.</param>
        public static StartupPhase? Identify(
            Bitmap screen, IEnumerable<StartupPhase> phases, out double difference)
        {
            if (screen == null)
            {
                throw new ArgumentNullException(nameof(screen));
            }

            if (phases == null)
            {
                throw new ArgumentNullException(nameof(phases));
            }

            // Phases can be recognised by different parts of the screen, so a fingerprint
            // is needed per region rather than one for the whole frame. There are a
            // handful of phases and at most as many distinct regions, so they are worked
            // out on demand and reused.
            var byRegion = new Dictionary<string, double[]>();
            StartupPhase? best = null;
            difference = 1.0;

            foreach (StartupPhase phase in phases)
            {
                string key = phase.Region == null ? "-" : phase.Region.Value.ToString();
                if (!byRegion.TryGetValue(key, out double[]? current))
                {
                    current = phase.Region == null
                        ? GameScreen.FingerprintOf(screen)
                        : GameScreen.FingerprintRegion(screen, phase.Region.Value);
                    byRegion[key] = current;
                }

                if (phase.Fingerprint.Length != current.Length)
                {
                    continue;
                }

                double distance = GameScreen.Difference(phase.Fingerprint, current);
                if (distance <= phase.Threshold && distance < difference)
                {
                    best = phase;
                    difference = distance;
                }
            }

            return best;
        }
    }
}
