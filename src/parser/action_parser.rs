// SPDX-License-Identifier: MIT
use crate::core::action::{DialogueAction, DialogueActionKind};
use crate::core::clock::ClockReading;
use crate::core::state::StateSymbols;
use crate::index::journal::Journal;

const ONCE_FN: &str = "once";

/// The letter that follows a backslash where a script separates two statements.
const SEPARATOR_ESCAPE: char = 'n';

/// What the game prefixes a reputation's dialogue variable with.
///
/// Not one of `StateSymbols`' synthetic namespaces. This is the real variable name the
/// guards read - `Variable["reputation.apocalypse_cop"]` - so it is interned as an
/// ordinary variable and the prefix only says how to build the name.
const REPUTATION_PREFIX: &str = "reputation.";

/// What an action that assigns a variable as a Lua statement, rather than through
/// `SetVariableValue`, is named in reports.
const DIRECT_ASSIGNMENT: &str = "Variable[] =";

/// Parse a userScript into DialogueActions, with no journal to resolve a task against.
///
/// For a script that names no task, and for tests: a journal action here resolves to nothing
/// and writes nothing. A graph built from the index uses [`parse_actions_with_journal`].
pub fn parse_actions(script: &str, symbols: &mut StateSymbols) -> Vec<DialogueAction> {
    parse_actions_with_journal(script, symbols, &Journal::default())
}

/// Parse a userScript into DialogueActions, resolving journal actions through `journal`.
pub fn parse_actions_with_journal(
    script: &str,
    symbols: &mut StateSymbols,
    journal: &Journal,
) -> Vec<DialogueAction> {
    let stripped = normalize(script);
    let mut actions = Vec::new();
    for statement in statements(&stripped) {
        // A STATEMENT THAT ASSIGNS A VARIABLE DIRECTLY, which is not a call and which the call
        // scan below would skip without a word: `Variable["tc.electronic_locks"] = true`. Five
        // scripts in the database write one.
        if let Some((variable, value)) = direct_assignment(statement) {
            let slot = symbols.variable(&variable);
            translate_value_write(slot, &variable, value, DIRECT_ASSIGNMENT, &mut actions);
            continue;
        }
        for call in invocations(statement) {
            translate_call(call, symbols, journal, &mut actions);
        }
    }
    actions
}

/// The statements of a normalised script, split at newlines and at `;` outside strings and
/// brackets.
///
/// SPLIT RATHER THAN SCANNED WHOLE only so a direct assignment can be told apart from the calls
/// around it while their order is kept - the order is the order the game applies them in.
fn statements(script: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in script.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' | '\n' if depth <= 0 => {
                found.push(&script[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    found.push(&script[start..]);
    found.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// `Variable["name"] = value` as a statement, split into the name and the value's text.
///
/// `==` is a comparison and is not one.
fn direct_assignment(statement: &str) -> Option<(String, &str)> {
    let rest = statement.trim().strip_prefix("Variable")?.trim_start();
    let rest = rest.strip_prefix('[')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let close = rest.find('"')?;
    let name = &rest[..close];
    let rest = rest[close + 1..]
        .trim_start()
        .strip_prefix(']')?
        .trim_start();
    let value = rest.strip_prefix('=')?;
    if value.starts_with('=') {
        return None;
    }
    Some((name.to_string(), value.trim()))
}

/// Strips comments and turns the statement separator into a real newline.
///
/// ## The separator is two characters, not one
///
/// The database stores a userScript as ONE LINE whose statements are separated by a
/// literal backslash followed by the letter `n` - two characters, not a newline. There
/// are 6,760 of them across the index.
///
/// Left as they stand, the name scanner in [`invocations`] starts one character late: a
/// backslash is not a name start and a letter is, so the separator is read into the name
/// of the call that follows it. `Start();\nSetVariableValue("x", true)` yields a call
/// named `nSetVariableValue`, which matches nothing in [`translate_call`] and lands as
/// unmodelled - and so does every statement after the first in every script. In
/// conversation 631's group that was 78 actions.
///
/// ## One pass, because Lua's rules interleave
///
/// A quote inside a comment opens no string and a `--` inside a string opens no comment,
/// so neither can be decided without tracking the other. Two passes get both wrong: the
/// prose these scripts carry is full of escaped quotes and the occasional em-dash.
fn normalize(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            out.push(c);
            if c == '\\' {
                // Whatever it escapes, quote included: a scan that read `\"` as the
                // closing quote would end the string in the middle of a sentence and
                // tokenize the rest of the prose as code.
                if let Some(&escaped) = chars.get(i + 1) {
                    out.push(escaped);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }

        if c == '\\' {
            // Outside a string an escape is the statement separator, which becomes the
            // newline it stands for. Anything else escaped out here is not Lua the
            // parser can read, so it goes the same way rather than being left to start
            // a name.
            out.push(if chars.get(i + 1) == Some(&SEPARATOR_ESCAPE) {
                '\n'
            } else {
                ' '
            });
            i += if i + 1 < chars.len() { 2 } else { 1 };
            continue;
        }

        if c == '-' && chars.get(i + 1) == Some(&'-') {
            i += 2;
            if chars.get(i) == Some(&'[') && chars.get(i + 1) == Some(&'[') {
                i += 2;
                let mut depth = 1;
                while i < chars.len() {
                    if chars[i] == ']' && chars.get(i + 1) == Some(&']') {
                        i += 2;
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if chars[i] == '[' && chars.get(i + 1) == Some(&'[') {
                        i += 2;
                        depth += 1;
                    } else {
                        i += 1;
                    }
                }
                out.push(' ');
                continue;
            }

            // A line comment, which ends at the separator. Looking for a real newline -
            // which no script contains - made one `--` eat the whole remainder.
            while i < chars.len() {
                if chars[i] == '\n' {
                    break;
                }
                if chars[i] == '\\' && chars.get(i + 1) == Some(&SEPARATOR_ESCAPE) {
                    i += 2;
                    break;
                }
                i += 1;
            }
            out.push('\n');
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

#[derive(Debug)]
struct Invocation {
    name: String,
    args: Vec<String>,
}

fn invocations(script: &str) -> Vec<Invocation> {
    let mut result = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = script.chars().collect();
    while i < chars.len() {
        while i < chars.len() && !is_name_start(chars[i]) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && is_name_part(chars[i]) {
            i += 1;
        }
        let name = chars[start..i].iter().collect::<String>();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '(' {
            continue;
        }
        let mut args = Vec::new();
        let mut current = String::new();
        // One, not zero: the call's own opening bracket is consumed just below, so the
        // scan starts already inside it. Starting at zero made the matching ')' take the
        // depth to -1 instead of 0, so the loop never broke and the first call in a
        // script swallowed every one after it - three statements parsed as one action.
        let mut depth = 1;
        let mut in_string = false;
        i += 1; // skip '('
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                current.push(c);
                // An escape carries its next character with it, so `\"` stays inside the
                // string instead of closing it - see `normalize`, which does the same for
                // the same reason.
                if c == '\\' {
                    if let Some(&escaped) = chars.get(i + 1) {
                        current.push(escaped);
                        i += 1;
                    }
                } else if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if c == '"' {
                in_string = true;
                current.push(c);
                i += 1;
                continue;
            }
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    break;
                }
            } else if c == ',' && depth == 1 {
                args.push(current.trim().to_string());
                current.clear();
                i += 1;
                continue;
            }
            current.push(c);
            i += 1;
        }
        if !current.is_empty() {
            args.push(current.trim().to_string());
        }
        result.push(Invocation { name, args });
    }
    result
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The action a write of `value`'s text to a variable is.
///
/// Shared by `SetVariableValue` and a direct `Variable[name] = value` statement, which the game
/// runs the same way. In order:
///
/// - an INCREMENT of the variable by itself, `Variable[name] + N` or `+ once(N)`;
/// - a CLOCK READING, `TotalHourCount() + N`, `DayCount()` or `NextMorningTime()`, which
///   scripts store as deadlines - see [`crate::core::clock::ClockReading`];
/// - a LITERAL, `true`, `false` or a number.
///
/// ANYTHING ELSE IS UNMODELLED rather than guessed: `not(Variable[...])` or a reputation
/// question stored as a value cannot be written as one number when the script is parsed, and a
/// guessed number would be a wrong value where unmodelled is a visible gap.
fn translate_value_write(
    slot: usize,
    variable: &str,
    value: &str,
    name: &str,
    actions: &mut Vec<DialogueAction>,
) {
    let value = value.trim();

    // Read the increment ONCE, keeping what it reports. This previously called
    // try_read_increment with throwaway temporaries to ask whether the value was
    // an increment, then called a helper that re-read it with an EMPTY variable
    // name - so the self-reference it looks for, Variable[""], was never found,
    // and every counter in the database became an increment of zero that had
    // also lost its once flag.
    let mut amount = 0;
    let mut once = false;
    if try_read_increment(value, variable, &mut amount, &mut once) {
        actions.push(DialogueAction::increment(
            slot,
            amount,
            once,
            name.to_string(),
        ));
        return;
    }
    if let Some((reading, offset)) = read_clock_value(value) {
        actions.push(DialogueAction::assign_clock(
            slot,
            reading,
            offset,
            name.to_string(),
        ));
        return;
    }
    match read_assigned_value(value) {
        Some(literal) => actions.push(DialogueAction::assign(slot, literal, name.to_string())),
        None => actions.push(DialogueAction::unmodelled(name.to_string())),
    }
}

/// `Reading()` or `Reading() + N` for a clock reading, as the reading and the offset.
fn read_clock_value(value: &str) -> Option<(ClockReading, i32)> {
    let open = value.find('(')?;
    let reading = ClockReading::called(value[..open].trim())?;
    let rest = value[open + 1..].trim_start().strip_prefix(')')?.trim();
    if rest.is_empty() {
        return Some((reading, 0));
    }
    let (sign, amount) = match rest.chars().next()? {
        '+' => (1, &rest[1..]),
        '-' => (-1, &rest[1..]),
        _ => return None,
    };
    Some((reading, sign * amount.trim().parse::<i32>().ok()?))
}

/// Actions a call's ARGUMENTS perform, which Lua runs before the call itself.
///
/// One script in the database hides a journal write in a value:
/// `SetVariableValue(..., true and CancelTask("TASK.become_man_of_plenty_cancelled"))`. Only
/// calls this parser models as writes are kept - a nested read such as `TotalHourCount()` or
/// `IsHighestCopotype(...)` is part of the value, not an action.
fn nested_writes(
    call: &Invocation,
    symbols: &mut StateSymbols,
    journal: &Journal,
    actions: &mut Vec<DialogueAction>,
) {
    for arg in &call.args {
        for nested in invocations(&code_of(arg)) {
            let mut found = Vec::new();
            translate_call(nested, symbols, journal, &mut found);
            actions.extend(found.into_iter().filter(|action| {
                !matches!(
                    action.kind(),
                    DialogueActionKind::Unmodelled | DialogueActionKind::Declared
                )
            }));
        }
    }
}

/// An argument with the strings at its own level blanked, so only code is scanned for calls.
///
/// A string argument is text, and the prose some carry quotes calls - `NewspaperEndgame`'s
/// reported speech includes `GainItem(\"x\")`. A string INSIDE a nested call's brackets is
/// that call's argument and is kept: `CancelTask("TASK.x")` needs its subject.
fn code_of(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for c in arg.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(if depth > 0 || c == '"' { c } else { ' ' });
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        out.push(c);
    }
    out
}

fn translate_call(
    call: Invocation,
    symbols: &mut StateSymbols,
    journal: &Journal,
    actions: &mut Vec<DialogueAction>,
) {
    nested_writes(&call, symbols, journal, actions);
    match call.name.as_str() {
        "SetVariableValue" => {
            if call.args.len() < 2 {
                actions.push(DialogueAction::unmodelled(call.name));
                return;
            }
            let var_name = unquote(&call.args[0]);
            let slot = symbols.variable(&var_name);
            translate_value_write(slot, &var_name, &call.args[1], &call.name, actions);
        }
        // A flag IS a dialogue variable. The game's Final Cut addition declares
        // `SetFlag(string variableName)` - the parameter name is the giveaway - alongside
        // `UnsetFlag` and a reader `FlagSet`, and the database bears it out: 62 scripts
        // call SetFlag, 9 guards call FlagSet, and 56 guard lines read the very names
        // SetFlag writes as `Variable[...]`.
        //
        // Left unmodelled, a path that opens only after a SetFlag stays closed for the
        // search, so it misses reachable states and can lose a marker - the failure that
        // shows nothing rather than something wrong.
        "SetFlag" | "UnsetFlag" => {
            let raised = call.name == "SetFlag";
            let slot = symbols.variable(&unquote(call.args.first().unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, i32::from(raised), call.name));
        }
        // Reputation is a dialogue variable under a prefix, and the game says so plainly.
        // `KarmaLuaFunctions.ReputationGrows` calls `ModifyOnce(name, 1)`, which reaches
        // `ReputationAlterant.ModifyReputation`, whose whole body is
        //
        //     Lua.Run("Variable[\"reputation.<name>\"] = Variable[\"reputation.<name>\"] + 1")
        //
        // wrapped in the same `once()` the search already models. `ReputationLowers` is the
        // same with -1.
        //
        // Not scorekeeping, whatever the name suggests: conversation 631's guards read
        // `Variable["reputation.apocalypse_cop"] >= 2`, so leaving these unmodelled holds
        // shut a branch that reputation opens. 71 calls in that group alone.
        //
        // `Reputation(name, amount)` is the same function with the step spelled out, and
        // the decompiled source says so rather than the name suggesting it:
        //
        //     Reputation(name, value)  -> ModifyRep(name, (int)value)
        //     ModifyOnce(name, value)  -> ModifyRep(name, (int)value)
        //     ModifyRep                -> ReputationAlterant.ReputationOption(name, value)
        //     ReputationOption         -> if (Once(value) != 0) Modify...(name, value)
        //
        // So all three run the same path, all three are wrapped in the same `once()`, and
        // ReputationGrows is exactly `Reputation(name, 1)`. It was the last unmodelled
        // action in the whole script corpus - twelve calls - and was left undecided
        // precisely because guessing between Modify and ModifyOnce would have been a
        // guess. It is ModifyOnce.
        "ReputationGrows" | "ReputationLowers" | "Reputation" => {
            let subject = unquote(call.args.first().unwrap_or(&String::new()));
            let slot = symbols.variable(&format!("{REPUTATION_PREFIX}{subject}"));
            let step = match call.name.as_str() {
                "ReputationGrows" => 1,
                "ReputationLowers" => -1,
                // The amount is the second argument, and the game reads it as an int.
                // An amount that will not parse is a script this cannot read, so it moves
                // nothing rather than moving by a guessed step.
                _ => call
                    .args
                    .get(1)
                    .and_then(|a| a.trim().parse::<i32>().ok())
                    .unwrap_or(0),
            };
            actions.push(DialogueAction::increment(slot, step, true, call.name));
        }
        // Awarding experience the first time and recording that it has been awarded.
        // `TaskLuaFunctions.XPSetBool` is
        //
        //     if not Variable[var] then Variable[var] = true; xp = xp + amount end
        //
        // The experience is not search state and the search has no use for it. The VARIABLE
        // is, and guards read it like any other. Assigning 1 unconditionally rather than
        // only when unset comes to the same thing, because the value is only ever 1.
        "XPPicoSetBool" | "XPTinySetBool" | "XPMinorSetBool" | "XPStandardSetBool"
        | "XPMajorSetBool" => {
            let slot = symbols.variable(&unquote(call.args.first().unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        "GainItem" => {
            let slot = symbols.item(&unquote(call.args.first().unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        // A lost item also leaves whatever equipment slot held it - see `core::equipment`.
        "LoseItem" => {
            let item = unquote(call.args.first().unwrap_or(&String::new()));
            actions.push(DialogueAction::assign(
                symbols.item(&item),
                0,
                call.name.clone(),
            ));
            actions.push(DialogueAction::assign(
                symbols.unequipped(&item),
                1,
                call.name,
            ));
        }
        // The only way dialogue writes the thought cabinet, and it writes exactly one
        // thing: the thought joins `gainedThoughts`, which is what `IsTHCPresent` reads.
        //
        // Assigning 1 unconditionally is exact rather than convenient.
        // `Inventory.CanBeGained` refuses a thought already gained, so a second call
        // changes nothing - and a thought the player has FORGOTTEN can never be regained,
        // which this does not model. That direction is safe: forgetting costs a skill
        // point and no search can do it, so a thought the save says is forgotten is one
        // the search was never going to be told about anyway.
        // DAMAGE AND HEALING, as the amount a `damage:` slot holds - see `core::damage`. A
        // heal in conversation goes through `Once`; an amount that will not parse moves
        // nothing rather than a guessed amount.
        name if crate::core::damage::skill_written_by(name).is_some() => {
            let skill = crate::core::damage::skill_written_by(name).expect("just matched");
            let slot = symbols.damage(skill);
            if name == "HealAllVolition" {
                actions.push(DialogueAction::assign(slot, 0, call.name));
                return;
            }
            let Some(amount) = call
                .args
                .first()
                .and_then(|a| a.trim().parse::<f64>().ok())
                .map(|a| a as i32)
            else {
                actions.push(DialogueAction::unmodelled(call.name));
                return;
            };
            if name.starts_with("Heal") {
                actions.push(DialogueAction::increment(slot, -amount, true, call.name));
            } else {
                actions.push(DialogueAction::increment(slot, amount, false, call.name));
            }
        }
        // KIM LEFT AT THE CHURCH, as the slot that answers the Kim questions false - see
        // `core::party`.
        name if crate::core::party::removes_kim(name) => {
            actions.push(DialogueAction::assign(symbols.kim_removed(), 1, call.name));
        }
        "GainThought" => {
            let slot = symbols.thought(&unquote(call.args.first().unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        // THE JOURNAL, as writes to the variables that ARE its state - see
        // `index::journal`. `JournalModel` resolves the argument to a task or subtask by any of
        // its three variables and, from the pre-final-cut export:
        //
        //     GainTask:   if (IsVisible || IsCanceled) return;  Reveal()  -> show = true
        //     FinishTask: if (IsDone) return;  if (!IsVisible) Reveal();  -> show = true
        //                 MarkDone()                                      -> done = true
        //     CancelTask: if (IsDone) return false;                       -> cancel = true
        //
        // A shown part is already true in its show variable, so "unless visible" needs no test
        // of its own. An argument naming no part writes nothing, as the game logs and returns.
        "GainTask" | "FinishTask" | "CancelTask" => {
            let named = unquote(call.args.first().unwrap_or(&String::new()));
            let Some((_, part)) = journal.part_named(&named) else {
                return;
            };
            let show = symbols.variable(&part.show);
            let done = symbols.variable(&part.done);
            match call.name.as_str() {
                "GainTask" => actions.push(match &part.cancel {
                    Some(cancel) => {
                        let cancel = symbols.variable(cancel);
                        DialogueAction::assign_unless(show, 1, cancel, call.name)
                    }
                    None => DialogueAction::assign(show, 1, call.name),
                }),
                "FinishTask" => {
                    actions.push(DialogueAction::assign_unless(
                        show,
                        1,
                        done,
                        call.name.clone(),
                    ));
                    actions.push(DialogueAction::assign(done, 1, call.name));
                }
                _ => {
                    if let Some(cancel) = &part.cancel {
                        let cancel = symbols.variable(cancel);
                        actions.push(DialogueAction::assign_unless(cancel, 1, done, call.name));
                    }
                }
            }
        }
        "GainMoneyOnce" | "GainMoneyAlways" | "LoseMoneyOnce" | "LoseMoneyAlways" => {
            let gain = call.name.starts_with("Gain");
            let once = call.name.ends_with("Once");
            let amount = call
                .args
                .first()
                .and_then(|s| s.trim().parse::<i32>().ok())
                .unwrap_or(0);
            actions.push(DialogueAction::money(gain, amount, once, call.name));
        }
        "PassTime" => {
            actions.push(DialogueAction::pass_time(call.name));
        }
        // Everything else either has a decision behind it or does not. A decision makes
        // it a stub that does nothing on purpose; the absence of one makes it a gap
        // nobody has looked at, and those are what the modelling-gaps report is for.
        name => {
            actions.push(match crate::core::modelling::for_action(name) {
                Some(_) => DialogueAction::declared(call.name),
                None => DialogueAction::unmodelled(call.name),
            });
        }
    }
}

fn try_read_increment(value: &str, variable: &str, amount: &mut i32, once: &mut bool) -> bool {
    let self_ref = format!("Variable[\"{variable}\"]");
    let Some(idx) = value.find(&self_ref) else {
        return false;
    };
    let rest = &value[idx + self_ref.len()..].trim();
    if !rest.starts_with('+') {
        return false;
    }
    let rest = &rest[1..].trim();
    *once = false;
    let rest = if rest.starts_with(ONCE_FN) {
        *once = true;
        if let Some(open) = rest.find('(') {
            if let Some(close) = rest.rfind(')') {
                &rest[open + 1..close]
            } else {
                return false;
            }
        } else {
            return false;
        }
    } else {
        rest
    };
    if let Ok(v) = rest.trim().parse::<i32>() {
        *amount = v;
        true
    } else {
        false
    }
}

/// A literal value as a slot holds it, or `None` for anything that is not a literal.
fn read_assigned_value(value: &str) -> Option<i32> {
    let t = value.trim();
    if t.eq_ignore_ascii_case("true") {
        return Some(1);
    }
    if t.eq_ignore_ascii_case("false") {
        return Some(0);
    }
    t.parse().ok()
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}
