// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Sends a command to the in-game probe by writing the file it watches.
    /// </summary>
    /// <remarks>
    /// <para>The file is written to a temporary name and then moved into place, because
    /// the probe polls with <c>File.Exists</c> and would otherwise be able to read a
    /// half-written command. A move on one volume is atomic, and the temporary name is a
    /// sibling so it is on the same volume by construction.</para>
    ///
    /// <para>Writing the file is not the same as the command having run. The probe
    /// deletes it before executing, so its disappearance only means it was read; what a
    /// caller waits on is the probe event the command produces, through
    /// <see cref="ProbeWatcher"/>.</para>
    /// </remarks>
    public static class ProbeCommand
    {
        /// <summary>The file the probe watches, inside the SaveGames folder.</summary>
        public const string FileName = "gct-probe-command.json";

        /// <summary>Load a savegame by name.</summary>
        public const string LoadSave = "load-save";

        /// <summary>Open a conversation, with no walking and no clicking.</summary>
        public const string StartConversation = "start-conversation";

        /// <summary>Ask the probe to report the state a scenario cares about.</summary>
        public const string Report = "report";

        /// <summary>Where the command file goes for a given profile.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <exception cref="ArgumentNullException">The folder is null.</exception>
        public static string PathIn(string saveGamesFolder)
        {
            if (saveGamesFolder == null)
            {
                throw new ArgumentNullException(nameof(saveGamesFolder));
            }

            return Path.Combine(saveGamesFolder, FileName);
        }

        /// <summary>Asks the probe to load a savegame.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="saveName">The save's name, without any extension.</param>
        public static void SendLoadSave(string saveGamesFolder, string saveName)
        {
            if (string.IsNullOrWhiteSpace(saveName))
            {
                throw new ArgumentException("A save name is needed.", nameof(saveName));
            }

            Send(saveGamesFolder, LoadSave, "save", saveName);
        }

        /// <summary>Asks the probe to start a conversation by id.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="conversationId">The conversation's database id.</param>
        public static void SendStartConversation(string saveGamesFolder, int conversationId)
        {
            Send(saveGamesFolder, StartConversation, "conversation", conversationId);
        }

        /// <summary>Asks the probe to report where the game currently is.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        public static void SendReport(string saveGamesFolder)
        {
            Send(saveGamesFolder, Report);
        }

        /// <summary>Writes one command.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <param name="command">The command name.</param>
        /// <param name="fields">Alternating name and value.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="InvalidOperationException">A command is still pending.</exception>
        public static void Send(
            string saveGamesFolder, string command, params object?[] fields)
        {
            if (command == null)
            {
                throw new ArgumentNullException(nameof(command));
            }

            string path = PathIn(saveGamesFolder);
            if (File.Exists(path))
            {
                // The probe deletes a command before running it, so one still sitting
                // there means it has not been picked up - the game is not running, the
                // probe is not loaded, or it is stalled. Overwriting would lose the
                // earlier command and report nothing about why.
                throw new InvalidOperationException(
                    $"A probe command is still pending at {path}. The game may not be "
                    + "running, or the probe may not be loaded.");
            }

            Directory.CreateDirectory(saveGamesFolder);

            // Written aside and moved in: the probe polls with File.Exists and would
            // otherwise be able to read a half-written command.
            string staging = path + ".writing";
            File.WriteAllText(staging, Render(command, fields), new UTF8Encoding(false));
            File.Move(staging, path);
        }

        /// <summary>Removes a command nobody picked up.</summary>
        /// <param name="saveGamesFolder">The profile's SaveGames folder.</param>
        /// <returns>Whether there was one to remove.</returns>
        public static bool Clear(string saveGamesFolder)
        {
            string path = PathIn(saveGamesFolder);
            if (!File.Exists(path))
            {
                return false;
            }

            File.Delete(path);
            return true;
        }

        private static string Render(string command, object?[] fields)
        {
            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                writer.WriteString("command", command);

                for (int i = 0; i + 1 < fields.Length; i += 2)
                {
                    object? value = fields[i + 1];
                    if (value == null)
                    {
                        continue;
                    }

                    string key = Convert.ToString(fields[i]) ?? string.Empty;
                    if (value is int number)
                    {
                        writer.WriteNumber(key, number);
                    }
                    else if (value is bool flag)
                    {
                        writer.WriteBoolean(key, flag);
                    }
                    else
                    {
                        writer.WriteString(key, Convert.ToString(value) ?? string.Empty);
                    }
                }

                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }
    }
}
