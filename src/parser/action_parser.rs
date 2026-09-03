// SPDX-License-Identifier: MIT
use crate::core::action::DialogueAction;
use crate::core::state::StateSymbols;

const ONCE_FN: &str = "once";

/// The letter that follows a backslash where a script separates two statements.
const SEPARATOR_ESCAPE: char = 'n';

/// What the game prefixes a reputation's dialogue variable with.
///
/// Not one of `StateSymbols`' synthetic namespaces. This is the real variable name the
/// guards read - `Variable["reputation.apocalypse_cop"]` - so it is interned as an
/// ordinary variable and the prefix only says how to build the name.
const REPUTATION_PREFIX: &str = "reputation.";

/// Parse a userScript into DialogueActions.
pub fn parse_actions(script: &str, symbols: &mut StateSymbols) -> Vec<DialogueAction> {
    let stripped = normalize(script);
    let mut actions = Vec::new();
    for call in invocations(&stripped) {
        translate_call(call, symbols, &mut actions);
    }
    actions
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
            out.push(if chars.get(i + 1) == Some(&SEPARATOR_ESCAPE) { '\n' } else { ' ' });
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
                        if depth == 0 { break; }
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
        while i < chars.len() && !is_name_start(chars[i]) { i += 1; }
        if i >= chars.len() { break; }
        let start = i;
        while i < chars.len() && is_name_part(chars[i]) { i += 1; }
        let name = chars[start..i].iter().collect::<String>();
        while i < chars.len() && chars[i].is_whitespace() { i += 1; }
        if i >= chars.len() || chars[i] != '(' { continue; }
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

fn translate_call(call: Invocation, symbols: &mut StateSymbols, actions: &mut Vec<DialogueAction>) {
    match call.name.as_str() {
        "SetVariableValue" => {
            if call.args.len() < 2 {
                actions.push(DialogueAction::unmodelled(call.name));
                return;
            }
            let var_name = unquote(&call.args[0]);
            let slot = symbols.variable(&var_name);
            let value = call.args[1].trim();

            // Read the increment ONCE, keeping what it reports. This previously called
            // try_read_increment with throwaway temporaries to ask whether the value was
            // an increment, then called a helper that re-read it with an EMPTY variable
            // name - so the self-reference it looks for, Variable[""], was never found,
            // and every counter in the database became an increment of zero that had
            // also lost its once flag.
            let mut amount = 0;
            let mut once = false;
            if try_read_increment(value, &var_name, &mut amount, &mut once) {
                actions.push(DialogueAction::increment(slot, amount, once, call.name));
                return;
            }
            let val = read_assigned_value(value);
            actions.push(DialogueAction::assign(slot, val, call.name));
        }
        // A flag IS a dialogue variable. The game's Final Cut addition declares
        // `SetFlag(string variableName)` - the parameter name is the giveaway - alongside
        // `UnsetFlag` and a reader `FlagSet`, and the database bears it out: 62 scripts
        // call SetFlag, 9 guards call FlagSet, and 56 guard lines read the very names
        // SetFlag writes as `Variable[...]`.
        //
        // Left unmodelled, a path that opens only after a SetFlag stays closed for the
        // crawl, so it misses reachable states and can lose a marker - the failure that
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
        // wrapped in the same `once()` the crawl already models. `ReputationLowers` is the
        // same with -1.
        //
        // Not scorekeeping, whatever the name suggests: conversation 631's guards read
        // `Variable["reputation.apocalypse_cop"] >= 2`, so leaving these unmodelled holds
        // shut a branch that reputation opens. 71 calls in that group alone.
        "ReputationGrows" | "ReputationLowers" => {
            let subject = unquote(call.args.first().unwrap_or(&String::new()));
            let slot = symbols.variable(&format!("{REPUTATION_PREFIX}{subject}"));
            let step = if call.name == "ReputationGrows" { 1 } else { -1 };
            actions.push(DialogueAction::increment(slot, step, true, call.name));
        }
        // Awarding experience the first time and recording that it has been awarded.
        // `TaskLuaFunctions.XPSetBool` is
        //
        //     if not Variable[var] then Variable[var] = true; xp = xp + amount end
        //
        // The experience is not crawl state and the crawl has no use for it. The VARIABLE
        // is, and guards read it like any other. Assigning 1 unconditionally rather than
        // only when unset comes to the same thing, because the value is only ever 1.
        "XPPicoSetBool" | "XPTinySetBool" | "XPMinorSetBool" | "XPStandardSetBool"
        | "XPMajorSetBool" => {
            let slot = symbols.variable(&unquote(call.args.first().unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        "GainItem" => {
            let slot = symbols.item(&unquote(call.args.get(0).unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        "LoseItem" => {
            let slot = symbols.item(&unquote(call.args.get(0).unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 0, call.name));
        }
        "GainTask" => {
            let slot = symbols.task(&unquote(call.args.get(0).unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 1, call.name));
        }
        "FinishTask" | "CancelTask" => {
            let slot = symbols.task(&unquote(call.args.get(0).unwrap_or(&String::new())));
            actions.push(DialogueAction::assign(slot, 0, call.name));
        }
        "GainMoneyOnce" | "GainMoneyAlways" | "LoseMoneyOnce" | "LoseMoneyAlways" => {
            let gain = call.name.starts_with("Gain");
            let once = call.name.ends_with("Once");
            let amount = call.args.get(0).and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(0);
            actions.push(DialogueAction::money(gain, amount, once, call.name));
        }
        "PassTime" => {
            actions.push(DialogueAction::pass_time(call.name));
        }
        _ => {
            actions.push(DialogueAction::unmodelled(call.name));
        }
    }
}

fn try_read_increment(value: &str, variable: &str, amount: &mut i32, once: &mut bool) -> bool {
    let self_ref = format!("Variable[\"{variable}\"]");
    let Some(idx) = value.find(&self_ref) else { return false; };
    let rest = &value[idx + self_ref.len()..].trim();
    if !rest.starts_with('+') { return false; }
    let rest = &rest[1..].trim();
    *once = false;
    let rest = if rest.starts_with(ONCE_FN) {
        *once = true;
        if let Some(open) = rest.find('(') {
            if let Some(close) = rest.rfind(')') {
                &rest[open+1..close]
            } else { return false; }
        } else { return false; }
    } else { rest };
    if let Ok(v) = rest.trim().parse::<i32>() {
        *amount = v;
        true
    } else { false }
}

fn read_assigned_value(value: &str) -> i32 {
    let t = value.trim();
    if t.eq_ignore_ascii_case("true") { return 1; }
    if t.eq_ignore_ascii_case("false") { return 0; }
    t.parse().unwrap_or(1)
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len()-1].to_string()
    } else { t.to_string() }
}
