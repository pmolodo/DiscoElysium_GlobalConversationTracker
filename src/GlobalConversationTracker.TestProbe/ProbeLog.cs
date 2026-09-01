// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using System.Text.Json;
using BepInEx.Logging;

namespace GlobalConversationTracker.TestProbe
{
    /// <summary>
    /// The one shape every probe event takes: a JSON object between two markers, so a
    /// harness can lift it out of a log it shares with everything else.
    /// </summary>
    /// <remarks>
    /// <para>JSON rather than <c>key=value</c> because the interesting fields are
    /// arbitrary prose carrying markup - a dialogue option's text, with the colour tags
    /// the mod wrapped round it - and prose in this game contains quotes, asterisks,
    /// angle brackets, colons and newlines. A format with a specification handles that;
    /// an ad-hoc one accumulates escaping bugs until a test fails on a line of dialogue
    /// rather than on the feature. It also nests, which a menu of options needs.</para>
    ///
    /// <para>The markers exist because the payload does not arrive alone. BepInEx
    /// prefixes every line it writes with a level and a source, several plugins share
    /// the file, and the game writes to it too. <see cref="Begin"/> and
    /// <see cref="End"/> bracket the object so a reader never has to guess whether a
    /// line is payload, and each is written as its own log call so BepInEx's prefix
    /// lands on the marker rather than inside the JSON.</para>
    /// </remarks>
    internal static class ProbeLog
    {
        /// <summary>The line that opens an event.</summary>
        internal const string Begin = "GCTPROBE-BEGIN";

        /// <summary>The line that closes one.</summary>
        internal const string End = "GCTPROBE-END";

        /// <summary>The JSON member naming the event.</summary>
        internal const string EventKey = "event";

        private static ManualLogSource? _log;

        /// <summary>Points the probe at a log. Call once, from plugin load.</summary>
        /// <param name="log">Where events go.</param>
        /// <exception cref="ArgumentNullException"><paramref name="log"/> is null.</exception>
        internal static void Attach(ManualLogSource log)
        {
            _log = log ?? throw new ArgumentNullException(nameof(log));
        }

        /// <summary>Writes an event whose members are simple values.</summary>
        /// <param name="name">The event name, written as the "event" member.</param>
        /// <param name="fields">Alternating name and value; a null value is skipped.</param>
        internal static void Write(string name, params object?[] fields)
        {
            Write(name, writer => WriteFields(writer, fields));
        }

        /// <summary>Writes an event that has structure of its own.</summary>
        /// <param name="name">The event name, written as the "event" member.</param>
        /// <param name="body">Writes the remaining members into the open object.</param>
        internal static void Write(string name, Action<Utf8JsonWriter> body)
        {
            ManualLogSource? log = _log;
            if (log == null)
            {
                return;
            }

            string json;
            try
            {
                json = Render(name, body);
            }
            catch (Exception ex)
            {
                // Not from the event being unrepresentable - every value is a string, an
                // integer or a bool - so this is a bug rather than data, and it is
                // reported outside the markers so no reader mistakes it for an event.
                log.LogWarning($"A probe event named '{name}' could not be rendered: {ex.Message}");
                return;
            }

            // Three calls, not one string with newlines: BepInEx prefixes what it is
            // handed, and a multi-line payload would come back with its prefix buried in
            // the middle of the JSON.
            log.LogMessage(Begin);
            log.LogMessage(json);
            log.LogMessage(End);
        }

        /// <summary>Writes alternating name/value pairs into an open object.</summary>
        /// <param name="writer">The open writer.</param>
        /// <param name="fields">Alternating name and value; a null value is skipped.</param>
        internal static void WriteFields(Utf8JsonWriter writer, params object?[] fields)
        {
            for (int i = 0; i + 1 < fields.Length; i += 2)
            {
                object? value = fields[i + 1];
                if (value == null)
                {
                    continue;
                }

                string key = Convert.ToString(fields[i]) ?? string.Empty;
                switch (value)
                {
                    case bool flag:
                        writer.WriteBoolean(key, flag);
                        break;
                    case int number:
                        writer.WriteNumber(key, number);
                        break;
                    case long number:
                        writer.WriteNumber(key, number);
                        break;
                    default:
                        writer.WriteString(key, Convert.ToString(value) ?? string.Empty);
                        break;
                }
            }
        }

        /// <summary>
        /// Reports a failure without letting it reach the game.
        /// </summary>
        /// <remarks>
        /// A probe that throws inside a hook would break the very run it exists to
        /// observe, and a test that fails because its instrument fell over teaches
        /// nobody anything. Every hook body is wrapped in this.
        /// </remarks>
        /// <param name="where">What was being observed.</param>
        /// <param name="error">What went wrong.</param>
        internal static void Failed(string where, Exception error)
        {
            Write("error", "at", where, "message", error.Message);
        }

        /// <summary>
        /// Leaves markup characters alone instead of escaping them.
        /// </summary>
        /// <remarks>
        /// Utf8JsonWriter's default encoder is HTML-safe, so it escapes the angle
        /// brackets of the colour tag wrapped round a look-ahead marker into their
        /// six-character unicode form. A reader decodes that back correctly either way,
        /// but the log is also read by people chasing a failure, and the tag is the
        /// thing they are looking for. Nothing here is ever interpolated into a
        /// document, so the relaxed encoder costs nothing.
        /// </remarks>
        private static readonly JsonWriterOptions Options = new JsonWriterOptions
        {
            Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        };

        private static string Render(string name, Action<Utf8JsonWriter> body)
        {
            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer, Options))
            {
                writer.WriteStartObject();
                writer.WriteString(EventKey, name);
                body(writer);
                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }
    }
}
