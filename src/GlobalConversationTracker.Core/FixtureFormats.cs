// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Core;

/// <summary>
/// The formats that describe a TEST FIXTURE rather than a player's data.
/// </summary>
/// <remarks>
/// <para>HERE, WITH THE HEADER ITSELF, because more than one project has to agree about
/// them and they are not any one project's to own: the harness builds in-game runs from
/// these tables, the offline tests run the same rows over the shipped index, and the check
/// that every committed fixture is at the current version has to know what the current
/// version IS. A name each of them spells for itself is a name that can drift.</para>
///
/// <para>They are stamped for the same reason every other file is. A reader handed a
/// document of the wrong KIND is in worse trouble than one handed an old version of the
/// right kind, because these shapes overlap: both tables are objects of arrays of objects,
/// so one read as the other parses and quietly yields nothing the reader wanted.</para>
/// </remarks>
public static class FixtureFormats
{
    /// <summary>The look-ahead scenarios, as suites of saves and the markers they draw.</summary>
    public const string ScenarioSuites = "scenario-suites";

    /// <summary>The shapes a check's Pass and Fail line can take.</summary>
    public const string BranchShapes = "branch-shapes";

    /// <summary>The version of both this build writes.</summary>
    /// <remarks>
    /// ONE VERSION FOR BOTH, since they are one definition split by shape rather than two
    /// unrelated formats - see the remarks at the top of either file. A change to what a
    /// scenario may say is a change to both readers whichever table it lands in.
    /// </remarks>
    public const int FormatVersion = 1;
}
