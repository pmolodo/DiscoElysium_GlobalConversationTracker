using System;
using BepInEx.Logging;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Sends the initialization path's log lines to BepInEx.
    /// </summary>
    internal sealed class BepInExUnifiedStateLog : IUnifiedStateLog
    {
        private readonly ManualLogSource _source;

        public BepInExUnifiedStateLog(ManualLogSource source)
        {
            _source = source ?? throw new ArgumentNullException(nameof(source));
        }

        public void Info(string message) => _source.LogMessage(message);

        public void Warning(string message) => _source.LogWarning(message);

        public void Error(string message) => _source.LogError(message);
    }
}
