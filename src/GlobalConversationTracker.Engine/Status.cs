// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// What an engine call reports. Zero is success; everything else is a reason.
    /// </summary>
    /// <remarks>
    /// <para>Kept in step with <c>Status</c> in <c>src/service.rs</c> by hand. There are
    /// seven of them and they are append-only, so a generator would cost more than it
    /// saved - but the names and numbers must match, and <c>BridgeTests</c> checks the ones
    /// a test can provoke.</para>
    ///
    /// <para>ONE SET OF NUMBERS FOR BOTH WAYS ACROSS. The Rust enum is the source: its
    /// discriminants are what the C ABI returns as an integer, and what a response over the
    /// pipe carries in its <c>status</c> field. A code that meant one thing over the ABI and
    /// another over the pipe would be the kind of mismatch where both sides look right in
    /// isolation, which is why neither restates them.</para>
    /// </remarks>
    public enum Status
    {
        /// <summary>The call succeeded.</summary>
        Ok = 0,

        /// <summary>
        /// The engine was asked something before it had been opened over an index.
        /// </summary>
        /// <remarks>
        /// It named a null or unissued handle when the engine was a library. It means the
        /// same thing now that the process is the handle: a call arrived before the open.
        /// </remarks>
        BadHandle = -1,

        /// <summary>An argument was null, or a string was not valid UTF-8.</summary>
        BadArgument = -2,

        /// <summary>The conversation index could not be read.</summary>
        IndexUnreadable = -3,

        /// <summary>Something panicked inside the engine. The call did nothing.</summary>
        Panic = -4,

        /// <summary>The index holds no such conversation.</summary>
        NoSuchConversation = -5,

        /// <summary>An answer could not be turned into JSON. Should not happen.</summary>
        SerialiseFailed = -6,
    }
}
