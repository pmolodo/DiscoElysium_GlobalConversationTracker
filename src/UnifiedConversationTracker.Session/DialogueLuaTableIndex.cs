namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// A replica of <c>PixelCrushers.DialogueSystem.DialogueLua.StringToTableIndex</c>,
    /// the game's rule for turning an arbitrary string into a Lua table key.
    /// </summary>
    /// <remarks>
    /// <para><b>Why a replica rather than a call.</b> This layer deliberately knows
    /// nothing about Unity or the Dialogue System so it can be unit tested without the
    /// game, and the rule is four character
    /// substitutions. Calling the game's own would also cost an IL2CPP crossing per
    /// conversation on a path whose whole point is to avoid them.</para>
    ///
    /// <para><b>It has to be exact.</b> The name of the Lua variable a savegame's
    /// compressed SimStatus blob lives under is
    /// <c>Variable["Conversation_SimX_" + StringToTableIndex(articyId)]</c>
    /// (<c>PersistentDataManager.ExpandSimStatusForConversation</c>, decompiled at
    /// pre-final-cut-assetripper-export/.../PersistentDataManager.cs:777-783), so a
    /// key this produces differently from the game is a key that will not be found.
    /// The game's own definition, at DialogueLua.cs:594-601, is
    /// <c>SpacesToUnderscores(DoubleQuotesToSingle(s.Replace('"', '_')))</c> followed
    /// by <c>-</c>, <c>(</c> and <c>)</c> each becoming <c>_</c>. The nested calls
    /// flatten to what is below, because the leading <c>Replace('"', '_')</c> has
    /// already removed every double quote before <c>DoubleQuotesToSingle</c> looks for
    /// one - so that call's escaping of quotes cannot fire, and only its newline and
    /// carriage-return handling survives.</para>
    ///
    /// <para><b>In practice the ids this is applied to change under it rarely.</b> An
    /// articy id is a hex string or a short alphanumeric token, so most pass through
    /// untouched; the ones that do not are those written with a leading <c>-</c>,
    /// which the game keys as a leading <c>_</c>. That is not a reason to skip the
    /// substitution: it is the observable difference between resolving a conversation
    /// and missing it.</para>
    /// </remarks>
    public static class DialogueLuaTableIndex
    {
        /// <summary>
        /// The Lua table key the game would use for <paramref name="value"/>.
        /// </summary>
        /// <param name="value">The raw string, typically an articy id.</param>
        /// <returns>
        /// The key, or the empty string for null or empty input - which is what the
        /// game returns too.
        /// </returns>
        public static string Of(string? value)
        {
            if (string.IsNullOrEmpty(value))
            {
                return string.Empty;
            }

            return value!
                .Replace('"', '_')
                .Replace("\n", "\\n")
                .Replace("\r", string.Empty)
                .Replace(' ', '_')
                .Replace('-', '_')
                .Replace('(', '_')
                .Replace(')', '_');
        }
    }
}
