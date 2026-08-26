// SPDX-License-Identifier: MIT
using NtwtfDecode;
using NtwtfDecode.Tests;
using GlobalConversationTracker;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace GlobalStateCheck.Tests;

/// <summary>
/// The Conversation -> <see cref="GlobalConversationState"/> projection: which
/// rows of the save's Conversation table become recorded statuses, which are
/// passed over, and which are loud enough to stop the read.
/// </summary>
/// <remarks>
/// <para>
/// Every fixture here is a blob synthesised by <see cref="LuaBlob"/>, so none of
/// this needs a game install or a real save.
/// </para>
/// <para>
/// The reader's "no Conversation table" guard has no test: <see
/// cref="LuaTableVisitor.ReadAllTables"/> always returns all five of
/// <see cref="RawDataParser.TableNames"/>, so a blob that decodes at all has a
/// Conversation table. The guard is there for a decoder that stops doing that.
/// </para>
/// </remarks>
public class SaveConversationReaderProjectionTests
{
    /// <summary>The conversation every skip test carries alongside the bad row.</summary>
    private const int GoodConversationId = 7;

    private const int GoodEntryId = 10;

    // -------------------------------------------------------------------
    // What is recorded
    // -------------------------------------------------------------------

    [Fact]
    public void Load_RecordsTheStatusOfEveryDialogEntry()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(
                (
                    "7",
                    Conversation(
                        ("10", Entry(SimStatusNames.WasDisplayed)),
                        ("11", Entry(SimStatusNames.WasOffered))
                    )
                ),
                ("8", Conversation(("3", Entry(SimStatusNames.WasOffered))))
            )
        );

        Assert.Equal(2, state.ConversationCount);
        Assert.Equal(3, state.EntryCount);
        Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(7, 10));
        Assert.Equal(SimStatus.WasOffered, state.GetStatus(7, 11));
        Assert.Equal(SimStatus.WasOffered, state.GetStatus(8, 3));
    }

    [Fact]
    public void Load_UntouchedEntry_IsNotRecordedAtAll()
    {
        // The state drops Untouched on merge, which is the point: a real save
        // holds ~113,000 entries and only ~1,500 of them are above Untouched.
        GlobalConversationState state = Project(
            LuaBlob.Table(
                (
                    "7",
                    Conversation(
                        ("10", Entry(SimStatusNames.WasDisplayed)),
                        ("12", Entry(SimStatusNames.Untouched))
                    )
                )
            )
        );

        Assert.Equal(1, state.EntryCount);
        Assert.False(state.TryGetStatus(7, 12, out SimStatus status));
        Assert.Equal(SimStatus.Untouched, status);
    }

    [Fact]
    public void Load_ConversationWhereEveryEntryIsUntouched_IsNotRecordedAtAll()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(("7", Conversation(("12", Entry(SimStatusNames.Untouched)))))
        );

        Assert.True(state.IsEmpty);
        Assert.False(state.ContainsConversation(7));
    }

    [Fact]
    public void Load_EmptyConversationTable_GivesAnEmptyState()
    {
        Assert.True(Project(new LuaTable()).IsEmpty);
    }

    // -------------------------------------------------------------------
    // Rows the projection passes over. Each of these carries a well-formed
    // conversation as well, so the assertion shows the reader skipped the bad
    // row rather than stopping at it.
    // -------------------------------------------------------------------

    [Fact]
    public void Load_ConversationKeyThatIsNotANumber_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(
                ("Alias", Conversation(("3", Entry(SimStatusNames.WasDisplayed)))),
                GoodConversation()
            )
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_ConversationRowThatIsNotATable_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(("9", "not a conversation"), GoodConversation())
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_ConversationWithNoDialogField_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(("9", LuaBlob.Table(("Title", ConversationTitle))), GoodConversation())
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_DialogFieldThatIsNotATable_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(
                (
                    "9",
                    LuaBlob.Table(
                        ("Title", ConversationTitle),
                        (SaveConversationReader.DialogFieldName, "not a dialog map")
                    )
                ),
                GoodConversation()
            )
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_DialogEntryKeyThatIsNotANumber_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(
                ("9", Conversation(("Alias", Entry(SimStatusNames.WasDisplayed)))),
                GoodConversation()
            )
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_DialogEntryThatIsNotATable_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(("9", Conversation(("3", "not an entry"))), GoodConversation())
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_DialogEntryWithNoSimStatusField_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(
                ("9", Conversation(("3", LuaBlob.Table(("Title", ConversationTitle))))),
                GoodConversation()
            )
        );

        AssertOnlyTheGoodConversation(state);
    }

    [Fact]
    public void Load_SimStatusThatIsNotAString_IsSkipped()
    {
        GlobalConversationState state = Project(
            LuaBlob.Table(("9", Conversation(("3", Entry(2L)))), GoodConversation())
        );

        AssertOnlyTheGoodConversation(state);
    }

    // -------------------------------------------------------------------
    // Rows the projection refuses
    // -------------------------------------------------------------------

    [Fact]
    public void Load_UnrecognisedSimStatus_ThrowsRatherThanDroppingTheEntry()
    {
        // An unknown status string means an assumption about the save format is
        // wrong, and this tool exists to notice exactly that kind of thing.
        const string Bogus = "WasWhispered";

        ArgumentException error = Assert.Throws<ArgumentException>(
            () => Project(LuaBlob.Table(("9", Conversation(("3", Entry(Bogus))))))
        );

        Assert.Contains(Bogus, error.Message, StringComparison.Ordinal);
    }

    // -------------------------------------------------------------------
    // Fixtures
    // -------------------------------------------------------------------

    private const string ConversationTitle = "Kim Kitsuragi";

    /// <summary>Reads a save whose Conversation table is the given one.</summary>
    private static GlobalConversationState Project(LuaTable conversations)
    {
        using var temp = new TempDirectory();
        string path = temp.WriteFile(
            "autosave" + SaveBlob.LuaExtension,
            LuaBlob.SerializeConversations(conversations)
        );

        return SaveConversationReader.Load(path, temp.Path, out _);
    }

    /// <summary>A conversation row, shaped like a save's: a Title and a Dialog map.</summary>
    private static LuaTable Conversation(params (object Key, object? Value)[] dialog) =>
        LuaBlob.Table(
            ("Title", ConversationTitle),
            (SaveConversationReader.DialogFieldName, LuaBlob.Table(dialog))
        );

    /// <summary>A dialogue entry row, which is a SimStatus and nothing this tool reads.</summary>
    private static LuaTable Entry(object? status) =>
        LuaBlob.Table((SaveConversationReader.SimStatusFieldName, status));

    /// <summary>A conversation with nothing wrong with it, for the skip tests.</summary>
    private static (object Key, object? Value) GoodConversation() =>
        (
            GoodConversationId.ToString(),
            Conversation((GoodEntryId.ToString(), Entry(SimStatusNames.WasDisplayed)))
        );

    private static void AssertOnlyTheGoodConversation(GlobalConversationState state)
    {
        Assert.Equal(1, state.EntryCount);
        Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(GoodConversationId, GoodEntryId));
    }
}
