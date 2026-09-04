// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Which of the game's special node types an entry is, as decided by
    /// <c>ReturnDialogueOptionValidator.IsEntryValid</c>.
    /// </summary>
    /// <remarks>
    /// <para>This is the second gate on a link. <c>ConversationModel</c> admits an entry
    /// when its <c>conditionsString</c> is true AND <c>isDialogueEntryValid</c> says so,
    /// and Disco Elysium points that delegate at a validator which dispatches on exactly
    /// these seven cases. A model that reads only <c>conditionsString</c> misses all of
    /// them, and check nodes routinely leave that field empty.</para>
    ///
    /// <para>Each is identified by the presence of a field on the entry, not by a type:
    /// <c>DifficultyPass</c>, <c>DifficultyRed</c>, <c>DifficultyWhite</c>,
    /// <c>DifficultyAtmo</c>, <c>HiddenTest</c>, <c>ClickCost</c>, <c>kim_watch</c>.</para>
    /// </remarks>
    public enum DialogueCheckKind
    {
        /// <summary>An ordinary entry, gated only by its condition.</summary>
        None = 0,

        /// <summary>
        /// A passive skill check (<c>DifficultyPass</c>): fires when skill plus six
        /// clears the difficulty. A failure does not end the branch - the game sets
        /// <c>falseConditionAction = "Passthrough"</c> on the entry as it evaluates it,
        /// so the conversation walks through to the children.
        /// </summary>
        Passive = 1,

        /// <summary>
        /// A red check (<c>DifficultyRed</c>): one shot. Offered until it has been
        /// resolved either way, tracked by its flag variable and that flag's
        /// <c>_failed</c> twin.
        /// </summary>
        Red = 2,

        /// <summary>
        /// A white check (<c>DifficultyWhite</c>): retryable. Offered until it has been
        /// passed, so only the success flag removes it.
        /// </summary>
        White = 3,

        /// <summary>
        /// An atmospheric check (<c>DifficultyAtmo</c>) whose outcome is fixed by an
        /// <c>AlwaysSucceed</c> field rather than rolled. Offered until it has been seen.
        /// </summary>
        Fake = 4,

        /// <summary>
        /// A developer test option (<c>HiddenTest</c>). Hidden outside developer mode,
        /// so in a normal playthrough it is never reachable at all.
        /// </summary>
        Test = 5,

        /// <summary>
        /// A Kim switchboard entry (<c>kim_watch</c>). Available while its condition
        /// holds, and - unless flagged <c>boolean_only</c> - only until it has been seen.
        /// </summary>
        KimSwitch = 6,
    }
}
