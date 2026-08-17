using Il2CppInterop.Runtime;
using Language.Lua;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// The two conversions everything that reads the game's Lua state needs, in one
    /// place so both readers - the walk of the master database and the interception of
    /// the savegame's compressed blobs - fold nil away the same way.
    /// </summary>
    internal static class LuaValues
    {
        /// <summary>
        /// A Lua value as a table, or null if it is nil or something else. Folds the
        /// nil-versus-null distinction away: <c>GetValue</c> returns
        /// <c>LuaNil.Nil</c> for an absent key rather than a null reference.
        /// </summary>
        internal static LuaTable? AsTable(LuaValue? value) => value?.TryCast<LuaTable>();

        /// <summary>
        /// A Lua value as its string, or null if it is nil or not a string. Same
        /// nil-folding as <see cref="AsTable"/>.
        /// </summary>
        internal static string? AsText(LuaValue? value) => value?.TryCast<LuaString>()?.Text;
    }
}
