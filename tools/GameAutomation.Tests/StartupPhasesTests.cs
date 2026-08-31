// SPDX-License-Identifier: MIT
using System;
using System.Drawing;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Recognising which startup screen is showing.</summary>
    public class StartupPhasesTests : IDisposable
    {
        private readonly string _root;

        public StartupPhasesTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-phases-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_root);
        }

        public void Dispose()
        {
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        private static Bitmap Solid(Color colour, int width = 320, int height = 200)
        {
            var bitmap = new Bitmap(width, height);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.Clear(colour);
            }

            return bitmap;
        }

        private static StartupPhase PhaseOf(
            string name, Color colour, double threshold = 0.05, Rectangle? region = null)
        {
            using Bitmap bitmap = Solid(colour);
            double[] print = region == null
                ? GameScreen.FingerprintOf(bitmap)
                : GameScreen.FingerprintRegion(bitmap, region.Value);
            return new StartupPhase(name, print, threshold, region);
        }

        [Fact]
        public void AScreenIsIdentifiedAsItsOwnPhase()
        {
            StartupPhase[] phases =
            {
                PhaseOf("dark", Color.FromArgb(20, 20, 20)),
                PhaseOf("grey", Color.FromArgb(128, 128, 128)),
                PhaseOf("light", Color.FromArgb(230, 230, 230)),
            };

            using Bitmap grey = Solid(Color.FromArgb(128, 128, 128));

            StartupPhase? found = StartupPhases.Identify(grey, phases, out double difference);

            Assert.NotNull(found);
            Assert.Equal("grey", found!.Name);
            Assert.True(difference < 0.01, $"expected a close match, got {difference}");
        }

        /// <summary>
        /// A screen matching nothing must come back as nothing, not as the nearest phase.
        /// A wrong phase would have the harness pressing keys at the wrong screen.
        /// </summary>
        [Fact]
        public void AnUnknownScreenIsNotForcedIntoAPhase()
        {
            StartupPhase[] phases = { PhaseOf("dark", Color.FromArgb(20, 20, 20)) };

            using Bitmap white = Solid(Color.White);

            Assert.Null(StartupPhases.Identify(white, phases, out _));
        }

        /// <summary>
        /// The logo animates in the middle of a still field, so it is recognised by the
        /// field. A phase with a region must ignore what happens outside it.
        /// </summary>
        [Fact]
        public void ARegionIgnoresWhatChangesOutsideIt()
        {
            var top = new Rectangle(0, 0, 320, 80);
            StartupPhase[] phases = { PhaseOf("logo", Color.FromArgb(42, 42, 42), region: top) };

            using Bitmap animating = Solid(Color.FromArgb(42, 42, 42));
            using (Graphics graphics = Graphics.FromImage(animating))
            {
                // Something large and moving, below the region.
                graphics.FillEllipse(Brushes.SteelBlue, 100, 120, 140, 60);
            }

            StartupPhase? found = StartupPhases.Identify(animating, phases, out _);

            Assert.NotNull(found);
            Assert.Equal("logo", found!.Name);
        }

        [Fact]
        public void PhasesRoundTripThroughAFile()
        {
            StartupPhase[] phases =
            {
                PhaseOf("dark", Color.FromArgb(20, 20, 20)),
                PhaseOf("logo", Color.FromArgb(42, 42, 42), 0.0746, new Rectangle(0, 0, 320, 80)),
            };

            string path = Path.Combine(_root, "phases.txt");
            StartupPhases.Save(path, phases);
            StartupPhase[] read = StartupPhases.Load(path);

            Assert.Equal(2, read.Length);
            Assert.Equal("dark", read[0].Name);
            Assert.Null(read[0].Region);
            Assert.Equal("logo", read[1].Name);
            Assert.Equal(new Rectangle(0, 0, 320, 80), read[1].Region);
            Assert.Equal(0.0746, read[1].Threshold, 4);
            Assert.Equal(phases[1].Fingerprint.Length, read[1].Fingerprint.Length);
        }

        /// <summary>A file read back must still identify the screens it was built from.</summary>
        [Fact]
        public void AReadBackFileStillIdentifies()
        {
            string path = Path.Combine(_root, "phases.txt");
            StartupPhases.Save(path, new[] { PhaseOf("grey", Color.FromArgb(128, 128, 128)) });

            using Bitmap grey = Solid(Color.FromArgb(128, 128, 128));

            Assert.NotNull(StartupPhases.Identify(grey, StartupPhases.Load(path), out _));
        }

        [Fact]
        public void AMalformedLineIsRefused()
        {
            string path = Path.Combine(_root, "bad.txt");
            File.WriteAllText(path, "onlyaname\n");

            Assert.Throws<InvalidDataException>(() => StartupPhases.Load(path));
        }

        [Fact]
        public void AMissingFileIsRefused()
        {
            Assert.Throws<FileNotFoundException>(
                () => StartupPhases.Load(Path.Combine(_root, "nope.txt")));
        }

        [Fact]
        public void NoPhasesMeansNothingIsIdentified()
        {
            using Bitmap any = Solid(Color.Gray);

            Assert.Null(StartupPhases.Identify(any, Array.Empty<StartupPhase>(), out _));
        }

        [Fact]
        public void NullArgumentsAreRefused()
        {
            using Bitmap any = Solid(Color.Gray);

            Assert.Throws<ArgumentNullException>(
                () => StartupPhases.Identify(null!, Array.Empty<StartupPhase>(), out _));
            Assert.Throws<ArgumentNullException>(() => StartupPhases.Identify(any, null!, out _));
        }

        /// <summary>
        /// The shipped file must load and describe the four screens the harness expects.
        /// </summary>
        [Fact]
        public void TheShippedPhasesAreTheFourStartupScreens()
        {
            string path = FindRepoFile(Path.Combine("testing", StartupPhases.DefaultFileName));
            StartupPhase[] phases = StartupPhases.Load(path);

            Assert.Equal(4, phases.Length);
            Assert.Contains(phases, p => p.Name == "loading");
            Assert.Contains(phases, p => p.Name == "legal-notice");
            Assert.Contains(phases, p => p.Name == "logo");
            Assert.Contains(phases, p => p.Name == "main-menu");

            // The logo animates in its middle, so it must be matched on a region.
            StartupPhase logo = Array.Find(phases, p => p.Name == "logo")!;
            Assert.NotNull(logo.Region);
        }

        private static string FindRepoFile(string relative)
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                string candidate = Path.Combine(directory.FullName, relative);
                if (File.Exists(candidate))
                {
                    return candidate;
                }

                directory = directory.Parent;
            }

            throw new FileNotFoundException($"Could not find {relative} above the test binaries.");
        }
    }
}
