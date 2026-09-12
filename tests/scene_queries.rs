// SPDX-License-Identifier: MIT
//! The scene the player is standing in, as a group's guards ask about it.
//!
//! ## Why these three are worth their own file
//!
//! `IsExterior`, `IsRaining` and `IsSnowing` are the only queries in the game that ask
//! about the SCENE rather than about the character, and they were the last ones an offline
//! world could not answer.
//!
//! What makes them sharp rather than merely missing is the SHAPE the writers used. Every
//! one of them appears as a complementary PAIR - one entry guarded by `IsExterior()` and
//! the next by `(IsExterior()) == false` - so exactly one of the two is ever reachable. A
//! query nobody answers reads as unknown, which is permissive, which makes BOTH reachable.
//! An unanswered scene query does not lose a line; it invents one.
//!
//! ## Where the answers come from
//!
//! THE AREA, for `IsExterior`. A save records the area it left the player in, and the game
//! decides outdoors by looking that area up in `ApplicationManager.ScenePropertiesList` -
//! so `testing/scenes.json` is that list, derived from the game's own asset by
//! `tools/derive-scene-properties.py`. Two of the game's thirty-seven scenes are outdoors.
//!
//! A LUA VARIABLE, for the weather: `auto.is_raining` and `auto.is_snowing`, which every
//! save's `Variable` table carries. Nothing in either JSON document mentions weather, and
//! the export cannot say what `IsRaining()` reads because its body is an IL2CPP stub - but
//! `ArcticSwimmerEasterEgg` watches `auto.is_snowing` for the same condition the dialogue
//! guards ask about, which is what named them.
//!
//! WHETHER THE GAME AGREES is not checkable here. An offline world that answers the way
//! this one does still has to be held against a running game, which is what the scene suite
//! in `suites.json` is for.

use std::collections::BTreeSet;

use lookahead_engine::bridge::WireValue;
use lookahead_engine::service::Service;

mod common;

/// The queries that ask about the scene, as the engine renders their keys.
const SCENE_QUERIES: [&str; 3] = ["IsExterior()", "IsRaining()", "IsSnowing()"];

/// The conversation whose group asks all three.
///
/// Kim's own, which is also where this phase started: it was marked differently in the game
/// and offline, and only one side's world had ever been written down.
const ASKS_ALL_THREE: i32 = 29;

/// The conversations whose guards name a scene query, found by reading the shipped index.
///
/// Written down because it is a fact about the GAME rather than about this build, and
/// because it is what says the feature is worth having: six conversations, eighteen
/// entries, nine complementary pairs. `tools/survey-scene-guards.py` prints them.
const GUARDED_CONVERSATIONS: [i32; 6] = [29, 530, 625, 1065, 1124, 1458];

/// A save the game left OUTDOORS, in Martinaise.
const OUTDOORS: &str = "at-trashcan";

/// The same moment moved indoors, onto the Whirling's ground floor.
///
/// MADE FOR THIS, and the pair to [`OUTDOORS`]: the two are the same day and the same hour,
/// so what a run finds different between them is the door and not the clock.
const INDOORS: &str = "scene-indoors";

/// And one the game left indoors on its own, which is a different moment entirely.
const ELSEWHERE_INDOORS: &str = "at-garte";

/// The same outdoor save, in the rain.
const RAINING: &str = "scene-raining";

/// And in the snow.
const SNOWING: &str = "scene-snowing";

#[test]
fn the_engine_asks_about_the_scene_where_the_guards_do() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let engine = Service::open(&path, None).expect("the index reads");

    let questions = engine.questions(ASKS_ALL_THREE).expect("the group builds");

    for query in SCENE_QUERIES {
        assert!(
            questions.queries.iter().any(|asked| asked == query),
            "conversation {ASKS_ALL_THREE}'s group does not ask {query}; it asks {:?}",
            questions.queries,
        );
    }
}

/// Every conversation whose guards name one still has it in its group's questions.
///
/// The list is the whole of what the shipped index holds, so this is what would notice a
/// trim that dropped a scene guard on its way into the index the mod ships.
#[test]
fn every_conversation_that_guards_on_the_scene_asks_about_it() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let engine = Service::open(&path, None).expect("the index reads");

    for conversation in GUARDED_CONVERSATIONS {
        let questions = engine
            .questions(conversation)
            .unwrap_or_else(|why| panic!("conversation {conversation}: {why:?}"));

        assert!(
            questions
                .queries
                .iter()
                .any(|asked| SCENE_QUERIES.contains(&asked.as_str())),
            "conversation {conversation}'s group asks about no part of the scene",
        );
    }
}

/// Two saves on opposite sides of one door answer `IsExterior` differently.
///
/// The offline world's half of the claim, read out of the committed saves themselves rather
/// than declared here: one is in Martinaise and one is in the Whirling, and the table
/// derived from the game says which of those the game calls outdoors.
#[test]
fn a_save_outdoors_and_a_save_indoors_answer_the_scene_differently() {
    let outdoors = common::fixtures::holdings_in_save(OUTDOORS);
    let indoors = common::fixtures::holdings_in_save(INDOORS);

    assert!(
        outdoors.scene.outside,
        "{OUTDOORS} is in {}, which the game does not call outdoors",
        outdoors.scene.area,
    );
    assert!(
        !indoors.scene.outside,
        "{INDOORS} is in {}, which the game calls outdoors",
        indoors.scene.area,
    );

    // AND NEITHER IS IN ANY WEATHER, which is what makes these two the dry pair. The saves
    // that are wet are made rather than played, and are named for it.
    for holdings in [&outdoors, &indoors] {
        assert!(
            !holdings.scene.raining && !holdings.scene.snowing,
            "{} is in weather, and neither of these two was",
            holdings.scene.area,
        );
    }

    // AND THEY STAGE THE SAME MOMENT, which is what makes the pair worth having. A guard
    // comparing the hour answers differently at another one, so a run that found a
    // different menu could not say whether the door or the clock did it.
    assert_eq!(
        (outdoors.day_counter, outdoors.day_minutes),
        (indoors.day_counter, indoors.day_minutes),
        "{OUTDOORS} and {INDOORS} are at different times, so they compare nothing cleanly",
    );

    // AND THEY HAVE READ THE SAME LINES. The clock is not the only thing that would muddy
    // the comparison: an entry shown in one and not the other moves a marker by itself, and
    // the indoor save was played rather than made, so it arrived having seen two lines the
    // outdoor one had not. Those were taken back out, and this is what keeps them out.
    assert_eq!(
        common::fixtures::read_in_save_group(OUTDOORS, &GUARDED_CONVERSATIONS),
        common::fixtures::read_in_save_group(INDOORS, &GUARDED_CONVERSATIONS),
        "{OUTDOORS} and {INDOORS} have shown different lines, so a marker that differed \
         between them could not be pinned on the door",
    );

    // A save the game left indoors of its own accord, so the table is not being read off
    // one area alone.
    assert!(
        !common::fixtures::holdings_in_save(ELSEWHERE_INDOORS)
            .scene
            .outside,
    );
}

/// The weather saves are in weather, and are otherwise the save they were made from.
///
/// MADE RATHER THAN PLAYED, which is what this has to protect. Waiting in game for the
/// weather to turn means letting the clock run, and the clock moves the day, the thoughts
/// cooking, and whatever else is on a timer - so two saves meant to differ in the sky would
/// differ in a dozen things and no run could say which one moved a marker.
#[test]
fn a_weather_save_differs_from_the_one_it_was_made_from_in_one_variable() {
    let dry = common::fixtures::holdings_in_save(OUTDOORS);
    let was = common::fixtures::variables_in_save(OUTDOORS);

    for (save, wet) in [(RAINING, "auto.is_raining"), (SNOWING, "auto.is_snowing")] {
        let scene = common::fixtures::holdings_in_save(save);
        let now = common::fixtures::variables_in_save(save);

        assert!(
            scene.scene.outside,
            "{save} is in {}, and the weather is only asked about outdoors",
            scene.scene.area,
        );
        assert_eq!(scene.scene.area, dry.scene.area, "{save} moved");

        // A KEY EITHER SIDE HOLDS, since a variable could have been added as well as
        // changed, and the set is what makes the claim rather than one map's view of it.
        let differing: BTreeSet<&str> = now
            .keys()
            .chain(was.keys())
            .map(String::as_str)
            .filter(|named| format!("{:?}", now.get(*named)) != format!("{:?}", was.get(*named)))
            .collect();

        assert_eq!(
            differing,
            BTreeSet::from([wet]),
            "{save} differs from {OUTDOORS} in something other than the weather",
        );
    }
}

/// And what differs is what the queries answer from.
#[test]
fn the_weather_saves_answer_the_weather_they_were_made_with() {
    let raining = common::fixtures::holdings_in_save(RAINING).scene;
    let snowing = common::fixtures::holdings_in_save(SNOWING).scene;

    assert!(raining.raining && !raining.snowing, "{raining:?}");
    assert!(snowing.snowing && !snowing.raining, "{snowing:?}");
}

/// The offline world leaves none of the three unanswered, for any committed save.
///
/// THE POINT OF THE WHOLE TICKET, and the shortest statement of it. An unanswered query is
/// not a missing line: it reads as unknown, unknown is permissive, and both halves of a
/// complementary pair then open - so a crawl reaches a line the game would never draw.
///
/// Held over EVERY committed save rather than one, because the answer comes out of the save
/// and a save that stopped recording its area would go unnoticed otherwise.
#[test]
fn no_committed_save_leaves_a_scene_query_unanswered() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let engine = Service::open(&path, None).expect("the index reads");
    let asked = engine.questions(ASKS_ALL_THREE).expect("the group builds");

    let scene: Vec<String> = asked
        .queries
        .iter()
        .filter(|query| SCENE_QUERIES.contains(&query.as_str()))
        .cloned()
        .collect();
    assert_eq!(
        scene.len(),
        SCENE_QUERIES.len(),
        "conversation {ASKS_ALL_THREE} asks {scene:?}, and this needs all three",
    );

    for save in common::fixtures::committed_saves() {
        let holdings = common::fixtures::holdings_in_save(&save);
        let answers = holdings.answers_to(&scene);

        let missing: Vec<&String> = scene
            .iter()
            .filter(|query| !answers.contains_key(*query))
            .collect();
        assert!(
            missing.is_empty(),
            "{save}, in {}, cannot answer {missing:?}",
            holdings.scene.area,
        );
    }
}

/// And what it answers is a boolean, which is what a guard compares against.
#[test]
fn the_scene_is_answered_as_something_a_guard_can_compare() {
    let holdings = common::fixtures::holdings_in_save(OUTDOORS);

    let answers = holdings.answers_to(&SCENE_QUERIES.map(str::to_string));
    let says = |query: &str| match answers.get(query) {
        Some(WireValue::Bool { value }) => *value,
        other => panic!("{query} is answered {other:?}, and a guard compares booleans"),
    };

    assert!(
        says(SCENE_QUERIES[0]),
        "{OUTDOORS} is in {}, which is outdoors",
        holdings.scene.area,
    );
    assert!(
        !says(SCENE_QUERIES[1]) && !says(SCENE_QUERIES[2]),
        "{OUTDOORS} is in no weather",
    );
}
