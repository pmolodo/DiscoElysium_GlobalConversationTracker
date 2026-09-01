// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Linq;
using System.Text.Json;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Sending commands to the in-game probe.</summary>
    public class ProbeCommandTests : IDisposable
    {
        private readonly string _saveGames;

        /// <summary>Makes a stand-in SaveGames folder.</summary>
        public ProbeCommandTests()
        {
            _saveGames = Path.Combine(
                Path.GetTempPath(), "gct-command-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_saveGames);
        }

        /// <summary>Removes it.</summary>
        public void Dispose()
        {
            if (Directory.Exists(_saveGames))
            {
                Directory.Delete(_saveGames, recursive: true);
            }
        }

        private string Written => File.ReadAllText(ProbeCommand.PathIn(_saveGames));

        private JsonElement Parsed()
        {
            using JsonDocument document = JsonDocument.Parse(Written);
            return document.RootElement.Clone();
        }

        [Fact]
        public void ALoadCommandNamesTheSave()
        {
            ProbeCommand.SendLoadSave(_saveGames, "afford-both");

            JsonElement body = Parsed();
            Assert.Equal(ProbeCommand.LoadSave, body.GetProperty("command").GetString());
            Assert.Equal("afford-both", body.GetProperty("save").GetString());
        }

        [Fact]
        public void AStartConversationCommandCarriesTheIdAsANumber()
        {
            ProbeCommand.SendStartConversation(_saveGames, 451);

            JsonElement body = Parsed();
            Assert.Equal(
                ProbeCommand.StartConversation, body.GetProperty("command").GetString());
            Assert.Equal(451, body.GetProperty("conversation").GetInt32());
        }

        [Fact]
        public void AReportCommandCarriesNothingElse()
        {
            ProbeCommand.SendReport(_saveGames);

            JsonElement body = Parsed();
            Assert.Equal(ProbeCommand.Report, body.GetProperty("command").GetString());
            Assert.Single(body.EnumerateObject());
        }

        [Fact]
        public void NoStagingFileIsLeftBehind()
        {
            ProbeCommand.SendLoadSave(_saveGames, "afford-both");

            Assert.Equal(
                new[] { ProbeCommand.FileName },
                Array.ConvertAll(
                    Directory.GetFiles(_saveGames), Path.GetFileName));
        }

        [Fact]
        public void APendingCommandIsRefusedRatherThanOverwritten()
        {
            // A command still sitting there means the probe has not picked it up: the
            // game is not running, or it is stalled. Overwriting would lose it and say
            // nothing about why.
            ProbeCommand.SendLoadSave(_saveGames, "afford-both");

            InvalidOperationException error = Assert.Throws<InvalidOperationException>(
                () => ProbeCommand.SendLoadSave(_saveGames, "afford-neither"));

            Assert.Contains("still pending", error.Message);
            Assert.Contains("afford-both", Written);
        }

        [Fact]
        public void ClearingLetsTheNextCommandThrough()
        {
            ProbeCommand.SendLoadSave(_saveGames, "afford-both");

            Assert.True(ProbeCommand.Clear(_saveGames));
            Assert.False(ProbeCommand.Clear(_saveGames));

            ProbeCommand.SendLoadSave(_saveGames, "afford-neither");
            Assert.Contains("afford-neither", Written);
        }

        [Fact]
        public void AMissingFolderIsCreated()
        {
            string nested = Path.Combine(_saveGames, "profile", "SaveGames");

            ProbeCommand.SendReport(nested);

            Assert.True(File.Exists(ProbeCommand.PathIn(nested)));
        }

        [Fact]
        public void TheProbeAndTheHarnessAgreeOnTheFileName()
        {
            // Two constants in two assemblies that must match; nothing but a test can
            // hold them together, since the probe cannot be referenced from here.
            Assert.Equal("gct-probe-command.json", ProbeCommand.FileName);
        }

        [Fact]
        public void NothingIsRefusedRatherThanWritten()
        {
            Assert.Throws<ArgumentNullException>(() => ProbeCommand.PathIn(null!));
            Assert.Throws<ArgumentNullException>(() => ProbeCommand.Send(_saveGames, null!));
            Assert.Throws<ArgumentException>(
                () => ProbeCommand.SendLoadSave(_saveGames, "  "));
        }
    }
}
