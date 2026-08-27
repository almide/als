//! Round-3 stdlib module: the ALS-D4 regex engine — transcribed verbatim
//! from the normative oracle `runtime/rs/src/regex.rs` (C-032: one canonical
//! grammar, fuzzed to byte identity across targets by
//! spec/wasm_cross/regex_engine.almd and regex_fuzz_batch.almd). The engine
//! is a backtracking matcher over chars: alternation with empty arms,
//! greedy `* + ? {n} {n,} {n,m}` (malformed braces stay literal), classes
//! with ranges and \d \w \s escapes, anchors, capture groups (index 0 is
//! the whole match), leftmost-first search, and the zero-width advance
//! rules of find_all / replace / split.

use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::value::Value;

// ---- Regex Runtime ----

#[derive(Clone)]
enum RxNode {
    Lit(char),
    Dot,
    Class(Vec<(char, char)>, bool), // ranges, negated
    AnchorStart,
    AnchorEnd,
    Group(Vec<Vec<RxPiece>>, usize), // alternations, capture index (1-based; 0 = no capture)
}

#[derive(Clone)]
struct RxPiece {
    node: RxNode,
    min: usize,
    max: Option<usize>,
}

struct RxPat {
    alts: Vec<Vec<RxPiece>>,
    ncap: usize,
}

type RxCaps = Vec<Option<(usize, usize)>>;

// ---- Parsing ----

fn rx_compile(pat: &str) -> RxPat {
    let chars: Vec<char> = pat.chars().collect();
    let mut pos = 0usize;
    let mut ncap = 0usize;
    let alts = rx_parse_alts(&chars, &mut pos, &mut ncap, false);
    RxPat { alts, ncap }
}

fn rx_parse_alts(
    chars: &[char],
    pos: &mut usize,
    ncap: &mut usize,
    in_group: bool,
) -> Vec<Vec<RxPiece>> {
    let mut alts: Vec<Vec<RxPiece>> = vec![vec![]];
    while *pos < chars.len() {
        if chars[*pos] == ')' && in_group {
            break;
        }
        if chars[*pos] == '|' {
            *pos += 1;
            alts.push(vec![]);
            continue;
        }
        let piece = rx_parse_piece(chars, pos, ncap);
        alts.last_mut().unwrap().push(piece);
    }
    alts
}

fn rx_parse_piece(chars: &[char], pos: &mut usize, ncap: &mut usize) -> RxPiece {
    let node = rx_parse_atom(chars, pos, ncap);
    let (min, max) = if *pos < chars.len() {
        match chars[*pos] {
            '*' => {
                *pos += 1;
                (0, None)
            }
            '+' => {
                *pos += 1;
                (1, None)
            }
            '?' => {
                *pos += 1;
                (0, Some(1))
            }
            '{' => {
                // {n}, {n,}, {n,m}. A malformed brace ({, {a}, {,3}) is left in
                // place and lexes as a literal `{` piece next — same fallback
                // most engines use.
                if let Some((min, max, consumed)) = rx_parse_brace(chars, *pos) {
                    *pos += consumed;
                    (min, max)
                } else {
                    (1, Some(1))
                }
            }
            _ => (1, Some(1)),
        }
    } else {
        (1, Some(1))
    };
    RxPiece { node, min, max }
}

/// Parse a `{n}` / `{n,}` / `{n,m}` quantifier starting at the `{` at `start`.
/// Returns (min, max, chars consumed including both braces), or None if the
/// brace expression is malformed (then the `{` stays a literal).
fn rx_parse_brace(chars: &[char], start: usize) -> Option<(usize, Option<usize>, usize)> {
    let mut i = start + 1; // past '{'
    let mut min_digits = String::new();
    while i < chars.len() && chars[i].is_ascii_digit() {
        min_digits.push(chars[i]);
        i += 1;
    }
    if min_digits.is_empty() {
        return None;
    }
    let min: usize = parse_usize(&min_digits)?;
    let max = if i < chars.len() && chars[i] == ',' {
        i += 1;
        let mut max_digits = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            max_digits.push(chars[i]);
            i += 1;
        }
        if max_digits.is_empty() {
            None
        } else {
            Some(parse_usize(&max_digits)?)
        }
    } else {
        Some(min)
    };
    if i < chars.len() && chars[i] == '}' {
        Some((min, max, i - start + 1))
    } else {
        None
    }
}

fn rx_parse_atom(chars: &[char], pos: &mut usize, ncap: &mut usize) -> RxNode {
    let c = chars[*pos];
    *pos += 1;
    match c {
        '.' => RxNode::Dot,
        '^' => RxNode::AnchorStart,
        '$' => RxNode::AnchorEnd,
        '\\' => rx_parse_escape(chars, pos),
        '[' => rx_parse_class(chars, pos),
        '(' => {
            *ncap += 1;
            let ci = *ncap;
            let alts = rx_parse_alts(chars, pos, ncap, true);
            if *pos < chars.len() && chars[*pos] == ')' {
                *pos += 1;
            }
            RxNode::Group(alts, ci)
        }
        _ => RxNode::Lit(c),
    }
}

fn rx_parse_escape(chars: &[char], pos: &mut usize) -> RxNode {
    if *pos >= chars.len() {
        return RxNode::Lit('\\');
    }
    let c = chars[*pos];
    *pos += 1;
    match c {
        'd' => RxNode::Class(vec![('0', '9')], false),
        'D' => RxNode::Class(vec![('0', '9')], true),
        'w' => RxNode::Class(vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')], false),
        'W' => RxNode::Class(vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')], true),
        's' => RxNode::Class(
            vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
            false,
        ),
        'S' => RxNode::Class(
            vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
            true,
        ),
        'n' => RxNode::Lit('\n'),
        't' => RxNode::Lit('\t'),
        'r' => RxNode::Lit('\r'),
        _ => RxNode::Lit(c),
    }
}

fn rx_parse_class(chars: &[char], pos: &mut usize) -> RxNode {
    let neg = *pos < chars.len() && chars[*pos] == '^';
    if neg {
        *pos += 1;
    }
    let mut ranges: Vec<(char, char)> = vec![];
    while *pos < chars.len() && chars[*pos] != ']' {
        if chars[*pos] == '\\' && *pos + 1 < chars.len() {
            *pos += 1;
            let esc = chars[*pos];
            *pos += 1;
            match esc {
                'd' => {
                    ranges.push(('0', '9'));
                    continue;
                }
                'w' => {
                    ranges.extend_from_slice(&[('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')]);
                    continue;
                }
                's' => {
                    ranges.extend_from_slice(&[
                        (' ', ' '),
                        ('\t', '\t'),
                        ('\n', '\n'),
                        ('\r', '\r'),
                    ]);
                    continue;
                }
                'D' => {
                    /* not fully supported in class, treat as literal */
                    ranges.push((esc, esc));
                    continue;
                }
                'n' => {
                    ranges.push(('\n', '\n'));
                    continue;
                }
                't' => {
                    ranges.push(('\t', '\t'));
                    continue;
                }
                _ => {
                    ranges.push((esc, esc));
                    continue;
                }
            }
        }
        let c = chars[*pos];
        *pos += 1;
        if *pos + 1 < chars.len() && chars[*pos] == '-' && chars[*pos + 1] != ']' {
            *pos += 1;
            let end = chars[*pos];
            *pos += 1;
            ranges.push((c, end));
        } else {
            ranges.push((c, c));
        }
    }
    if *pos < chars.len() {
        *pos += 1;
    } // skip ]
    RxNode::Class(ranges, neg)
}

// ---- Matching ----

fn rx_node_matches(node: &RxNode, c: char) -> bool {
    match node {
        RxNode::Lit(ch) => c == *ch,
        RxNode::Dot => c != '\n',
        RxNode::Class(ranges, neg) => {
            let hit = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
            hit != *neg
        }
        _ => false,
    }
}

fn rx_match_alts(alts: &[Vec<RxPiece>], s: &[char], p: usize, caps: &mut RxCaps) -> Option<usize> {
    for alt in alts {
        let save = caps.clone();
        if let Some(e) = rx_match_seq(alt, 0, s, p, caps) {
            return Some(e);
        }
        *caps = save;
    }
    None
}

fn rx_match_seq(
    seq: &[RxPiece],
    si: usize,
    s: &[char],
    p: usize,
    caps: &mut RxCaps,
) -> Option<usize> {
    if si >= seq.len() {
        return Some(p);
    }
    let piece = &seq[si];
    match &piece.node {
        RxNode::AnchorStart => {
            if p == 0 {
                rx_match_seq(seq, si + 1, s, p, caps)
            } else {
                None
            }
        }
        RxNode::AnchorEnd => {
            if p == s.len() {
                rx_match_seq(seq, si + 1, s, p, caps)
            } else {
                None
            }
        }
        _ => rx_match_rep(seq, si, s, p, caps, 0),
    }
}

fn rx_match_rep(
    seq: &[RxPiece],
    si: usize,
    s: &[char],
    p: usize,
    caps: &mut RxCaps,
    count: usize,
) -> Option<usize> {
    let piece = &seq[si];
    let at_max = piece.max.is_some_and(|m| count >= m);
    // Greedy: try to match one more first
    if !at_max {
        let save = caps.clone();
        if let Some(consumed) = rx_match_one(&piece.node, s, p, caps) {
            if consumed > 0 || count == 0 {
                // prevent infinite loop on zero-width
                if let Some(e) = rx_match_rep(seq, si, s, p + consumed, caps, count + 1) {
                    return Some(e);
                }
            }
        }
        *caps = save;
    }
    // Try rest of sequence if we have enough repetitions
    if count >= piece.min {
        return rx_match_seq(seq, si + 1, s, p, caps);
    }
    None
}

fn rx_match_one(node: &RxNode, s: &[char], p: usize, caps: &mut RxCaps) -> Option<usize> {
    match node {
        RxNode::Lit(_) | RxNode::Dot | RxNode::Class(_, _) => {
            if p < s.len() && rx_node_matches(node, s[p]) {
                Some(1)
            } else {
                None
            }
        }
        RxNode::Group(alts, ci) => {
            let start = p;
            if let Some(end) = rx_match_alts(alts, s, p, caps) {
                if *ci > 0 {
                    while caps.len() < *ci {
                        caps.push(None);
                    }
                    caps[*ci - 1] = Some((start, end));
                }
                Some(end - p)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---- Search ----

fn rx_find_at(rx: &RxPat, s: &[char], start: usize) -> Option<(usize, usize, RxCaps)> {
    for i in start..=s.len() {
        let mut caps: RxCaps = vec![None; rx.ncap];
        if let Some(end) = rx_match_alts(&rx.alts, s, i, &mut caps) {
            return Some((i, end, caps));
        }
    }
    None
}

// ---- Public API ----

pub fn almide_regex_is_match(pat: &str, s: &str) -> bool {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    rx_find_at(&rx, &chars, 0).is_some()
}

pub fn almide_regex_full_match(pat: &str, s: &str) -> bool {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut caps: RxCaps = vec![None; rx.ncap];
    if let Some(end) = rx_match_alts(&rx.alts, &chars, 0, &mut caps) {
        end == chars.len()
    } else {
        false
    }
}

pub fn almide_regex_find(pat: &str, s: &str) -> Option<String> {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    rx_find_at(&rx, &chars, 0).map(|(start, end, _)| chars[start..end].iter().collect())
}

pub fn almide_regex_find_all(pat: &str, s: &str) -> Vec<String> {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut results: Vec<String> = vec![];
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some((start, end, _)) = rx_find_at(&rx, &chars, pos) {
            results.push(chars[start..end].iter().collect());
            pos = if end > start { end } else { end + 1 };
        } else {
            break;
        }
    }
    results
}

pub fn almide_regex_replace(pat: &str, s: &str, rep: &str) -> String {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some((start, end, _)) = rx_find_at(&rx, &chars, pos) {
            result.extend(&chars[pos..start]);
            result.push_str(rep);
            pos = if end > start {
                end
            } else {
                // Zero-width match: emit the char at `end` and step past it so the
                // search advances. At end-of-string there is no char to emit
                // (`end == chars.len()`); guard the index so we don't panic and
                // simply advance past the end to terminate the loop.
                if end < chars.len() {
                    result.push(chars[end]);
                }
                end + 1
            };
        } else {
            result.extend(&chars[pos..]);
            break;
        }
    }
    result
}

pub fn almide_regex_replace_first(pat: &str, s: &str, rep: &str) -> String {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    if let Some((start, end, _)) = rx_find_at(&rx, &chars, 0) {
        let mut result = String::new();
        result.extend(&chars[..start]);
        result.push_str(rep);
        result.extend(&chars[end..]);
        result
    } else {
        s.to_string()
    }
}

pub fn almide_regex_split(pat: &str, s: &str) -> Vec<String> {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut results: Vec<String> = vec![];
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some((start, end, _)) = rx_find_at(&rx, &chars, pos) {
            if end == start && start == pos {
                // Zero-width match at current position: take one char and move on
                if pos < chars.len() {
                    results.push(chars[pos..pos + 1].iter().collect());
                    pos += 1;
                } else {
                    break;
                }
                continue;
            }
            results.push(chars[pos..start].iter().collect());
            pos = end;
        } else {
            results.push(chars[pos..].iter().collect());
            break;
        }
    }
    results
}

/// Index 0 is the WHOLE match, 1.. are the groups — the shape every mainstream
/// regex API uses (Rust `Captures::get(0)`, Python `m.group(0)`, JS `match[0]`,
/// PCRE), and the one `docs/stdlib/regex.md` always documented. The groups-only
/// return this replaced made `caps[1]` silently mean the SECOND group to anyone
/// carrying the universal habit over (almide#1432).
///
/// `None` now means exactly one thing: the pattern did not match. It previously
/// also came back for a pattern with NO groups, conflating "matched nothing to
/// capture" with "did not match" — with a full match at index 0 that case has a
/// real answer, `some([whole])`.
pub fn almide_regex_captures(pat: &str, s: &str) -> Option<Vec<String>> {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let (mstart, mend, caps) = rx_find_at(&rx, &chars, 0)?;
    let mut result = vec![chars[mstart..mend].iter().collect::<String>()];
    result.extend(caps.iter().map(|c| match c {
        Some((start, end)) => chars[*start..*end].iter().collect(),
        None => String::new(),
    }));
    Some(result)
}

/// digit fold without host str::parse (ADR-0015 clause 5); overflow → None,
/// which leaves the brace literal exactly like a malformed one
fn parse_usize(digits: &str) -> Option<usize> {
    let mut acc: usize = 0;
    for c in digits.chars() {
        acc = acc
            .checked_mul(10)?
            .checked_add((c as u32).checked_sub('0' as u32)? as usize)?;
    }
    Some(acc)
}

// ---- stdlib dispatch ----

pub const REGEX_FNS: &[&str] = &[
    "regex.is_match",
    "regex.full_match",
    "regex.find",
    "regex.find_all",
    "regex.replace",
    "regex.replace_first",
    "regex.split",
    "regex.captures",
];

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() == n {
        Ok(())
    } else {
        Err(Flow::Fatal(format!(
            "{name} takes {n} argument(s), got {}",
            args.len()
        )))
    }
}

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a str, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(Flow::Fatal(format!(
            "{name}: expected String, got {}",
            other.type_name()
        ))),
    }
}

pub fn call_regex(_it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !REGEX_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(name, args))
}

fn dispatch(name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        "regex.is_match" | "regex.full_match" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_str(name, &args[0])?, want_str(name, &args[1])?);
            Ok(Value::Bool(if name.ends_with("is_match") {
                almide_regex_is_match(p, s)
            } else {
                almide_regex_full_match(p, s)
            }))
        }
        "regex.find" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_str(name, &args[0])?, want_str(name, &args[1])?);
            Ok(match almide_regex_find(p, s) {
                Some(m) => Value::Some(Rc::new(Value::str(&m))),
                None => Value::None,
            })
        }
        "regex.find_all" | "regex.split" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_str(name, &args[0])?, want_str(name, &args[1])?);
            let items = if name.ends_with("find_all") {
                almide_regex_find_all(p, s)
            } else {
                almide_regex_split(p, s)
            };
            Ok(Value::List(Rc::new(
                items.iter().map(|x| Value::str(x)).collect(),
            )))
        }
        "regex.replace" | "regex.replace_first" => {
            arity(name, &args, 3)?;
            let (p, s, rep) = (
                want_str(name, &args[0])?,
                want_str(name, &args[1])?,
                want_str(name, &args[2])?,
            );
            Ok(Value::str(&if name.ends_with("_first") {
                almide_regex_replace_first(p, s, rep)
            } else {
                almide_regex_replace(p, s, rep)
            }))
        }
        "regex.captures" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_str(name, &args[0])?, want_str(name, &args[1])?);
            Ok(match almide_regex_captures(p, s) {
                Some(groups) => Value::Some(Rc::new(Value::List(Rc::new(
                    groups.iter().map(|x| Value::str(x)).collect(),
                )))),
                None => Value::None,
            })
        }
        other => Err(Flow::Fatal(format!("unrouted regex fn `{other}`"))),
    }
}
