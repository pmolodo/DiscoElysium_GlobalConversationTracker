// SPDX-License-Identifier: MIT
using System;
using BepInEx.Logging;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Sends the initialization path's log lines to BepInEx.
    /// </summary>
    internal sealed class BepInExGlobalStateLog : IGlobalStateLog
    {
        private readonly ManualLogSource _source;

        public BepInExGlobalStateLog(ManualLogSource source)
        {
            _source = source ?? throw new ArgumentNullException(nameof(source));
        }

        public void Info(string message) => _source.LogMessage(message);

        public void Warning(string message) => _source.LogWarning(message);

        public void Error(string message) => _source.LogError(message);
    }
}
