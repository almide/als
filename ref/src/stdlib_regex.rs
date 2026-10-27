//! Round-3 stdlib module: the ALS-D4 regex engine — transcribed verbatim
//! from the normative oracle `runtime/rs/src/regex.rs` (C-032: one canonical
//! grammar, fuzzed to byte identity across targets by
//! spec/wasm_cross/regex_engine.almd and regex_fuzz_batch.almd). The engine
//! is a backtracking matcher over chars: alternation with empty arms,
//! greedy and lazy `* + ? {n} {n,} {n,m}` (malformed braces stay literal),
//! classes with ranges and \d \w \s escapes, anchors (`(?m)` line anchors),
//! \b \B word boundaries, capturing and `(?:` groups (index 0 is the whole
//! match), leftmost-first search, and the zero-width advance rules of
//! find_all / replace / split.

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
    WordBoundary(bool),              // true = \b, false = \B
    Group(Vec<Vec<RxPiece>>, usize), // alternations, capture index (1-based; 0 = no capture)
}

#[derive(Clone)]
struct RxPiece {
    node: RxNode,
    min: usize,
    max: Option<usize>,
    lazy: bool,
}

struct RxPat {
    alts: Vec<Vec<RxPiece>>,
    ncap: usize,
    multiline: bool, // a leading (?m): ^ and $ also match at line breaks
}

type RxCaps = Vec<Option<(usize, usize)>>;

// ---- Parsing ----

fn rx_compile(pat: &str) -> RxPat {
    let chars: Vec<char> = pat.chars().collect();
    let mut pos = 0usize;
    let mut ncap = 0usize;
    // a leading flag group (C-395 accepts it only there): `i` folds case,
    // `m` lets ^ and $ also match at a line break
    let (mut multiline, mut icase) = (false, false);
    if let Some((len, m, ic)) = rx_flag_group(&chars) {
        pos = len;
        multiline = m;
        icase = ic;
    }
    let mut alts = rx_parse_alts(&chars, &mut pos, &mut ncap, false);
    if icase {
        rx_fold_case(&mut alts);
    }
    RxPat {
        alts,
        ncap,
        multiline,
    }
}

/// A leading `(?` [im]+ `)`: (its length in chars, whether it holds `m`,
/// whether it holds `i`).
fn rx_flag_group(chars: &[char]) -> Option<(usize, bool, bool)> {
    if chars.len() < 2 || chars[0] != '(' || chars[1] != '?' {
        return None;
    }
    let mut i = 2;
    let (mut m, mut ic) = (false, false);
    while i < chars.len() && (chars[i] == 'i' || chars[i] == 'm') {
        if chars[i] == 'i' {
            ic = true;
        } else {
            m = true;
        }
        i += 1;
    }
    if i > 2 && i < chars.len() && chars[i] == ')' {
        Some((i + 1, m, ic))
    } else {
        None
    }
}

/// `(?i)`: an ASCII letter matches in either case — a literal letter becomes
/// the class of both cases, and a class gains the other case of each letter
/// in it before its `^` negates it.
fn rx_fold_case(alts: &mut [Vec<RxPiece>]) {
    fn other(c: char) -> char {
        if c.is_ascii_uppercase() {
            char::from(c as u8 + 32)
        } else if c.is_ascii_lowercase() {
            char::from(c as u8 - 32)
        } else {
            c
        }
    }
    for seq in alts.iter_mut() {
        for piece in seq.iter_mut() {
            match &mut piece.node {
                RxNode::Lit(c) if c.is_ascii_alphabetic() => {
                    let o = other(*c);
                    piece.node = RxNode::Class(vec![(*c, *c), (o, o)], false);
                }
                RxNode::Class(ranges, _) => {
                    let mut extra = Vec::new();
                    for &(lo, hi) in ranges.iter() {
                        for (a, z) in [('a', 'z'), ('A', 'Z')] {
                            let l = if lo > a { lo } else { a };
                            let h = if hi < z { hi } else { z };
                            if l <= h {
                                extra.push((other(l), other(h)));
                            }
                        }
                    }
                    ranges.extend(extra);
                }
                RxNode::Group(inner, _) => rx_fold_case(inner),
                _ => {}
            }
        }
    }
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
    let atom_end = *pos;
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
    // a `?` after a quantifier makes it lazy (shortest first)
    let lazy = *pos > atom_end && *pos < chars.len() && chars[*pos] == '?';
    if lazy {
        *pos += 1;
    }
    RxPiece {
        node,
        min,
        max,
        lazy,
    }
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
            // `(?:` opens a group that captures nothing
            let ci = if chars[*pos..].starts_with(&['?', ':']) {
                *pos += 2;
                0
            } else {
                *ncap += 1;
                *ncap
            };
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
        'b' => RxNode::WordBoundary(true),
        'B' => RxNode::WordBoundary(false),
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
    // a `]` in the FIRST position is a literal member, not the terminator
    if *pos < chars.len() && chars[*pos] == ']' {
        ranges.push((']', ']'));
        *pos += 1;
    }
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

// The backtracking walk of the oracle (#2307): leftmost alternative first,
// greedy quantifiers longest-first, lazy quantifiers shortest-first, an empty
// repetition instance counts but never loops, and a group's capture is
// recorded when the group ends and restored when the walk backtracks past
// that point. The state lives in three vectors (untried choices, group-end
// frames, the capture trail), so the host stack stays flat whatever the
// input's length. The oracle's Pike VM gives the same answers in linear time;
// the evaluator walks without a step budget.

/// The continuation that ends the walk: the whole pattern has matched.
const RX_DONE: usize = usize::MAX;

/// A position in the pattern: piece `si` of the sequence `seq`.
#[derive(Clone, Copy)]
struct RxPc<'a> {
    seq: &'a [RxPiece],
    si: usize,
}

impl<'a> RxPc<'a> {
    fn piece(self) -> &'a RxPiece {
        &self.seq[self.si]
    }

    fn next(self) -> Self {
        RxPc {
            seq: self.seq,
            si: self.si + 1,
        }
    }
}

/// One step of the walk. `k` is the continuation: a frame index, or DONE.
#[derive(Clone, Copy)]
enum RxGoal<'a> {
    Seq {
        pc: RxPc<'a>,
        p: usize,
        k: usize,
    },
    Alts {
        alts: &'a [Vec<RxPiece>],
        ai: usize,
        p: usize,
        k: usize,
    },
    Inst {
        pc: RxPc<'a>,
        p: usize,
        count: usize,
        k: usize,
    },
    Lazy {
        pc: RxPc<'a>,
        p: usize,
        count: usize,
        k: usize,
    },
    Back {
        pc: RxPc<'a>,
        start: usize,
        count: usize,
        k: usize,
    },
    Run {
        k: usize,
        p: usize,
    },
    Fail,
}

/// The continuation after instance `count` of the repeated group at `pc`,
/// which began at `start`.
#[derive(Clone, Copy)]
struct RxFrame<'a> {
    pc: RxPc<'a>,
    start: usize,
    count: usize,
    next: usize,
}

#[derive(Clone, Copy)]
struct RxChoice<'a> {
    goal: RxGoal<'a>,
    frames: usize,
    trail: usize,
}

struct RxVm<'a> {
    s: &'a [char],
    multiline: bool,
    /// full_match: a match must end exactly here
    end_at: Option<usize>,
    caps: RxCaps,
    frames: Vec<RxFrame<'a>>,
    choices: Vec<RxChoice<'a>>,
    trail: Vec<(usize, Option<(usize, usize)>)>,
}

fn rx_is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn rx_at_boundary(s: &[char], p: usize) -> bool {
    let before = p > 0 && rx_is_word(s[p - 1]);
    let after = p < s.len() && rx_is_word(s[p]);
    before != after
}

impl<'a> RxVm<'a> {
    fn new(rx: &RxPat, s: &'a [char], end_at: Option<usize>) -> Self {
        RxVm {
            s,
            multiline: rx.multiline,
            end_at,
            caps: vec![None; rx.ncap],
            frames: Vec::new(),
            choices: Vec::new(),
            trail: Vec::new(),
        }
    }

    fn zero_width(&self, node: &RxNode, p: usize) -> Option<bool> {
        match node {
            RxNode::AnchorStart => Some(p == 0 || (self.multiline && self.s[p - 1] == '\n')),
            RxNode::AnchorEnd => Some(p == self.s.len() || (self.multiline && self.s[p] == '\n')),
            RxNode::WordBoundary(want) => Some(rx_at_boundary(self.s, p) == *want),
            _ => None,
        }
    }

    /// The end of the first match the walk reaches from `p`, if any; `caps`
    /// then holds that match's groups.
    fn exec(&mut self, alts: &'a [Vec<RxPiece>], p: usize) -> Option<usize> {
        self.frames.clear();
        self.choices.clear();
        self.trail.clear();
        for c in self.caps.iter_mut() {
            *c = None;
        }
        let mut goal = self.alts(alts, 0, p, RX_DONE);
        loop {
            goal = match goal {
                RxGoal::Seq { pc, p, k } => self.seq(pc, p, k),
                RxGoal::Alts { alts, ai, p, k } => self.alts(alts, ai, p, k),
                RxGoal::Inst { pc, p, count, k } => self.inst(pc, p, count, k),
                RxGoal::Lazy { pc, p, count, k } => self.lazy(pc, p, count, k),
                RxGoal::Run { k: RX_DONE, p } if self.end_at.is_none_or(|e| e == p) => {
                    return Some(p)
                }
                RxGoal::Run { k, p } if k != RX_DONE => self.resume(k, p),
                _ => self.backtrack()?,
            }
        }
    }

    fn push(&mut self, goal: RxGoal<'a>) {
        self.choices.push(RxChoice {
            goal,
            frames: self.frames.len(),
            trail: self.trail.len(),
        });
    }

    /// Resume the newest untried alternative, first undoing every capture
    /// write and dropping every frame made since it was left.
    fn backtrack(&mut self) -> Option<RxGoal<'a>> {
        let top = self.choices.len().checked_sub(1)?;
        let RxChoice {
            goal,
            frames,
            trail,
        } = self.choices[top];
        while self.trail.len() > trail {
            if let Some((slot, saved)) = self.trail.pop() {
                self.caps[slot] = saved;
            }
        }
        self.frames.truncate(frames);
        if let RxGoal::Back {
            pc,
            start,
            count,
            k,
        } = goal
        {
            // give back one more char next time, down to the minimum
            if count > pc.piece().min {
                self.choices[top].goal = RxGoal::Back {
                    pc,
                    start,
                    count: count - 1,
                    k,
                };
            } else {
                self.choices.pop();
            }
            return Some(RxGoal::Seq {
                pc: pc.next(),
                p: start + count,
                k,
            });
        }
        self.choices.pop();
        Some(goal)
    }

    fn alts(&mut self, alts: &'a [Vec<RxPiece>], ai: usize, p: usize, k: usize) -> RxGoal<'a> {
        let Some(seq) = alts.get(ai) else {
            return RxGoal::Fail;
        };
        if ai + 1 < alts.len() {
            self.push(RxGoal::Alts {
                alts,
                ai: ai + 1,
                p,
                k,
            });
        }
        RxGoal::Seq {
            pc: RxPc { seq, si: 0 },
            p,
            k,
        }
    }

    fn seq(&mut self, mut pc: RxPc<'a>, p: usize, k: usize) -> RxGoal<'a> {
        while let Some(piece) = pc.seq.get(pc.si) {
            match self.zero_width(&piece.node, p) {
                Some(true) => pc.si += 1,
                Some(false) => return RxGoal::Fail,
                None if matches!(piece.node, RxNode::Group(..)) => return self.rep(pc, p, 0, k),
                None if piece.lazy => return self.lazy(pc, p, 0, k),
                None => return self.greedy(pc, p, k),
            }
        }
        RxGoal::Run { k, p }
    }

    fn atom_at(&self, pc: RxPc<'a>, p: usize) -> bool {
        self.s
            .get(p)
            .is_some_and(|&c| rx_node_matches(&pc.piece().node, c))
    }

    /// A greedy single atom takes as many chars as it can, hands over to the
    /// rest of the sequence, and leaves one choice that gives them back.
    fn greedy(&mut self, pc: RxPc<'a>, p: usize, k: usize) -> RxGoal<'a> {
        let piece = pc.piece();
        let max = piece.max.unwrap_or(usize::MAX);
        let mut n = 0;
        while n < max && self.atom_at(pc, p + n) {
            n += 1;
        }
        if n < piece.min {
            return RxGoal::Fail;
        }
        if n > piece.min {
            self.push(RxGoal::Back {
                pc,
                start: p,
                count: n - 1,
                k,
            });
        }
        RxGoal::Seq {
            pc: pc.next(),
            p: p + n,
            k,
        }
    }

    /// A lazy single atom hands over to the rest as soon as it has its
    /// minimum, leaving one choice that takes one more char.
    fn lazy(&mut self, pc: RxPc<'a>, mut p: usize, mut count: usize, k: usize) -> RxGoal<'a> {
        let piece = pc.piece();
        let max = piece.max.unwrap_or(usize::MAX);
        loop {
            let more = count < max && self.atom_at(pc, p);
            if count >= piece.min {
                if more {
                    self.push(RxGoal::Lazy {
                        pc,
                        p: p + 1,
                        count: count + 1,
                        k,
                    });
                }
                return RxGoal::Seq {
                    pc: pc.next(),
                    p,
                    k,
                };
            }
            if !more {
                return RxGoal::Fail;
            }
            p += 1;
            count += 1;
        }
    }

    /// The repeated group at `pc` has matched `count` instances ending at
    /// `p`: greedy tries one more instance before the rest, lazy the rest
    /// first.
    fn rep(&mut self, pc: RxPc<'a>, p: usize, count: usize, k: usize) -> RxGoal<'a> {
        let piece = pc.piece();
        let more = piece.max.is_none_or(|m| count < m);
        let stop = RxGoal::Seq {
            pc: pc.next(),
            p,
            k,
        };
        match (count >= piece.min, more, piece.lazy) {
            (false, false, _) => RxGoal::Fail,
            (true, false, _) => stop,
            (false, true, _) => self.inst(pc, p, count, k),
            (true, true, true) => {
                self.push(RxGoal::Inst { pc, p, count, k });
                stop
            }
            (true, true, false) => {
                self.push(stop);
                self.inst(pc, p, count, k)
            }
        }
    }

    /// One instance of the group at `pc`: its alternatives, each continuing
    /// into a frame that records the capture and loops.
    fn inst(&mut self, pc: RxPc<'a>, p: usize, count: usize, k: usize) -> RxGoal<'a> {
        let RxNode::Group(alts, _) = &pc.piece().node else {
            return RxGoal::Fail;
        };
        let frame = self.frames.len();
        self.frames.push(RxFrame {
            pc,
            start: p,
            count: count + 1,
            next: k,
        });
        self.alts(alts, 0, p, frame)
    }

    /// A group instance ended at `p`: record its capture, then loop or stop.
    fn resume(&mut self, k: usize, p: usize) -> RxGoal<'a> {
        let f = self.frames[k];
        // the newest frame with no choice above it has no other reader left
        if k + 1 == self.frames.len() && k >= self.choices.last().map_or(0, |c| c.frames) {
            self.frames.pop();
        }
        let piece = f.pc.piece();
        if let RxNode::Group(_, ci) = piece.node {
            if ci > 0 {
                self.capture(ci - 1, f.start, p);
            }
        }
        if p != f.start {
            return self.rep(f.pc, p, f.count, f.next);
        }
        // an empty instance counts, but never loops
        if f.count < piece.min {
            RxGoal::Fail
        } else {
            RxGoal::Seq {
                pc: f.pc.next(),
                p,
                k: f.next,
            }
        }
    }

    fn capture(&mut self, slot: usize, start: usize, end: usize) {
        if !self.choices.is_empty() {
            self.trail.push((slot, self.caps[slot]));
        }
        self.caps[slot] = Some((start, end));
    }
}

// ---- Search ----

fn rx_find_at<'a>(rx: &'a RxPat, s: &'a [char], start: usize) -> Option<(usize, usize, RxCaps)> {
    let mut vm = RxVm::new(rx, s, None);
    for i in start..=s.len() {
        if let Some(end) = vm.exec(&rx.alts, i) {
            return Some((i, end, vm.caps.clone()));
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
    let mut vm = RxVm::new(&rx, &chars, Some(chars.len()));
    vm.exec(&rx.alts, 0).is_some()
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

/// A field is the text BETWEEN two matches (#2129): walk the matches as
/// find_all does (an empty match advances the scan by one char), emit the
/// text from the previous match's end to this match's start, and always emit
/// the trailing field, empty or not.
pub fn almide_regex_split(pat: &str, s: &str) -> Vec<String> {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut results: Vec<String> = vec![];
    let (mut last, mut scan) = (0usize, 0usize);
    while scan <= chars.len() {
        let Some((start, end, _)) = rx_find_at(&rx, &chars, scan) else {
            break;
        };
        results.push(chars[last..start].iter().collect());
        last = end;
        scan = if end > start { end } else { end + 1 };
    }
    results.push(chars[last..].iter().collect());
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

// ---- Validation (C-395, ALS-D4) ----
//
// `regex.new` / `regex.compile` accept a pattern only in the one grammar of
// ALS-D4 and with at most RX_MAX_STATES states: its Pike VM program's
// instructions (a counted repetition contributes one copy of its body per
// count) times one more than the deepest nesting of repeated groups that can
// match empty, counted over the pattern without expanding it. The first
// violation met left to right is the reason, worded as C-395 lists them.

const RX_MAX_STATES: u64 = 20_000;
const RX_MAX_NEST: usize = 250;

#[derive(Clone, Copy)]
struct RxSize {
    n: u64,
    nullable: bool,
    depth: u64,
}

fn rx_sat(x: u64) -> u64 {
    if x > RX_MAX_STATES + 1 {
        RX_MAX_STATES + 1
    } else {
        x
    }
}

/// past the `}` of a well-formed `{n}` / `{n,}` / `{n,m}` at `start`
fn rx_brace_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == start + 1 || i >= chars.len() {
        return None;
    }
    if chars[i] == '}' {
        return Some(i + 1);
    }
    if chars[i] != ',' {
        return None;
    }
    i += 1;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '}' {
        Some(i + 1)
    } else {
        None
    }
}

fn rx_count(chars: &[char], from: usize, to: usize) -> u64 {
    let mut acc = 0u64;
    for c in &chars[from..to] {
        acc = rx_sat(acc * 10 + (*c as u64 - '0' as u64));
    }
    acc
}

/// the reason `pat` is refused, or None
pub fn rx_check(pat: &str) -> Option<String> {
    let chars: Vec<char> = pat.chars().collect();
    let mut pos = rx_flag_group(&chars).map_or(0, |(len, _, _)| len);
    let size = match rx_check_alts(&chars, &mut pos, 0) {
        Ok(size) => size,
        Err(reason) => return Some(reason),
    };
    if pos < chars.len() {
        return Some("unmatched )".to_string());
    }
    if rx_sat(size.n + 1) * (size.depth + 1) > RX_MAX_STATES {
        return Some("pattern too large: more than 20000 states".to_string());
    }
    None
}

fn rx_check_alts(chars: &[char], pos: &mut usize, nest: usize) -> Result<RxSize, String> {
    let mut size = RxSize {
        n: 0,
        nullable: false,
        depth: 0,
    };
    let mut arms = 0u64;
    loop {
        let mut seq = RxSize {
            n: 0,
            nullable: true,
            depth: 0,
        };
        while *pos < chars.len() && chars[*pos] != '|' && chars[*pos] != ')' {
            let piece = rx_check_piece(chars, pos, nest)?;
            seq.n = rx_sat(seq.n + piece.n);
            seq.nullable = seq.nullable && piece.nullable;
            seq.depth = seq.depth.max(piece.depth);
        }
        arms += 1;
        size.n = rx_sat(size.n + seq.n);
        size.nullable = size.nullable || seq.nullable;
        size.depth = size.depth.max(seq.depth);
        if *pos < chars.len() && chars[*pos] == '|' {
            *pos += 1;
        } else {
            break;
        }
    }
    size.n = rx_sat(size.n + 2 * (arms - 1));
    Ok(size)
}

fn rx_group_syntax(chars: &[char], at: usize) -> String {
    let next = chars.get(at).copied();
    let after = chars.get(at + 1).copied();
    match next {
        Some('P') if after == Some('<') => "named groups are not supported",
        Some('<') if after != Some('=') && after != Some('!') => "named groups are not supported",
        Some('<') | Some('=') | Some('!') => "lookaround is not supported",
        Some('i') | Some('m') => "flags are only supported at the start of the pattern",
        _ => "unsupported group syntax",
    }
    .to_string()
}

fn rx_check_piece(chars: &[char], pos: &mut usize, nest: usize) -> Result<RxSize, String> {
    // 0 = one character, 1 = zero-width, 2 = a group
    let c = chars[*pos];
    let mut kind = 0;
    let mut body = RxSize {
        n: 0,
        nullable: false,
        depth: 0,
    };
    let mut capturing = false;
    match c {
        '*' | '+' | '?' => return Err("nothing to repeat".to_string()),
        '{' => {
            return Err(if rx_brace_end(chars, *pos).is_some() {
                "nothing to repeat".to_string()
            } else {
                "malformed repetition".to_string()
            });
        }
        '(' => {
            *pos += 1;
            if *pos < chars.len() && chars[*pos] == '?' {
                if chars.get(*pos + 1) != Some(&':') {
                    return Err(rx_group_syntax(chars, *pos + 1));
                }
                *pos += 2;
            } else {
                capturing = true;
            }
            if nest + 1 > RX_MAX_NEST {
                return Err("groups nested deeper than 250".to_string());
            }
            body = rx_check_alts(chars, pos, nest + 1)?;
            if *pos >= chars.len() {
                return Err("unclosed group".to_string());
            }
            *pos += 1;
            kind = 2;
        }
        '[' => {
            *pos += 1;
            rx_check_class(chars, pos)?;
        }
        '\\' => {
            if *pos + 1 >= chars.len() {
                return Err("trailing backslash".to_string());
            }
            let e = chars[*pos + 1];
            *pos += 2;
            match e {
                'b' | 'B' => kind = 1,
                'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'n' | 't' | 'r' => {}
                _ if e.is_ascii_alphanumeric() => {
                    let mut r = String::from("unsupported escape \\");
                    r.push(e);
                    return Err(r);
                }
                _ => {}
            }
        }
        '^' | '$' => {
            *pos += 1;
            kind = 1;
        }
        _ => *pos += 1,
    }
    let (mut min, mut max) = (1u64, Some(1u64));
    if *pos < chars.len() {
        let mut quantified = true;
        match chars[*pos] {
            '*' => (min, max) = (0, None),
            '+' => (min, max) = (1, None),
            '?' => (min, max) = (0, Some(1)),
            '{' => match rx_brace_end(chars, *pos) {
                None => return Err("malformed repetition".to_string()),
                Some(end) => {
                    let close = end - 1;
                    let mut comma = None;
                    for (k, ch) in chars.iter().enumerate().take(close).skip(*pos) {
                        if *ch == ',' {
                            comma = Some(k);
                            break;
                        }
                    }
                    min = rx_count(chars, *pos + 1, comma.unwrap_or(close));
                    max = match comma {
                        None => Some(min),
                        Some(k) if k + 1 == close => None,
                        Some(k) => Some(rx_count(chars, k + 1, close)),
                    };
                    if let Some(m) = max {
                        if min > m {
                            return Err("repetition minimum exceeds maximum".to_string());
                        }
                    }
                    *pos = end - 1;
                }
            },
            _ => quantified = false,
        }
        if quantified {
            *pos += 1;
            if *pos < chars.len() && chars[*pos] == '?' {
                *pos += 1;
            }
        }
    }
    Ok(match kind {
        1 => RxSize {
            n: 1,
            nullable: true,
            depth: 0,
        },
        0 => RxSize {
            n: rx_sat(match max {
                None => min + 3,
                Some(m) => min + 2 * (m - min),
            }),
            nullable: min == 0,
            depth: 0,
        },
        _ => {
            let copies = rx_sat(max.unwrap_or(min + 1));
            let splits = match max {
                None => 1,
                Some(m) => m - min,
            };
            let per = body.n + 1 + u64::from(capturing);
            let tracked = (min, max) != (1, Some(1)) && body.nullable;
            RxSize {
                n: rx_sat(copies * per + rx_sat(splits)),
                nullable: min == 0 || body.nullable,
                // a {0} repetition emits no copy: the groups inside it add
                // no nesting, the group itself still counts
                depth: if max == Some(0) {
                    u64::from(tracked)
                } else {
                    body.depth + u64::from(tracked)
                },
            }
        }
    })
}

fn rx_check_class(chars: &[char], pos: &mut usize) -> Result<(), String> {
    if *pos < chars.len() && chars[*pos] == '^' {
        *pos += 1;
    }
    if *pos < chars.len() && chars[*pos] == ']' {
        *pos += 1;
    }
    loop {
        if *pos >= chars.len() {
            return Err("unclosed character class".to_string());
        }
        let c = chars[*pos];
        if c == ']' {
            *pos += 1;
            return Ok(());
        }
        if c == '\\' {
            if *pos + 1 >= chars.len() {
                return Err("unclosed character class".to_string());
            }
            let e = chars[*pos + 1];
            *pos += 2;
            if !matches!(e, 'd' | 'w' | 's' | 'n' | 't') && e.is_ascii_alphanumeric() {
                let mut r = String::from("unsupported escape \\");
                r.push(e);
                r.push_str(" in a class");
                return Err(r);
            }
            continue;
        }
        *pos += 1;
        if *pos + 1 < chars.len() && chars[*pos] == '-' && chars[*pos + 1] != ']' {
            let hi = chars[*pos + 1];
            if hi == '\\' || c > hi {
                return Err("invalid class range".to_string());
            }
            *pos += 2;
        }
    }
}

/// a capture as the regex module hands it on: the text, or none for a group
/// that took no part (R-regex-08)
fn rx_opt(chars: &[char], span: Option<(usize, usize)>) -> Value {
    match span {
        Some((a, b)) => Value::Some(Rc::new(Value::str(&chars[a..b].iter().collect::<String>()))),
        None => Value::None,
    }
}

/// every match the way split and replace walk them: (field before it, its
/// captures [whole, group 1, ...]), then the trailing field
#[allow(clippy::type_complexity)]
fn rx_scan(pat: &str, s: &str, first: bool) -> (Vec<(String, Vec<Value>)>, String) {
    let rx = rx_compile(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let (mut last, mut scan) = (0usize, 0usize);
    while scan <= chars.len() {
        let Some((start, end, caps)) = rx_find_at(&rx, &chars, scan) else {
            break;
        };
        let mut groups = vec![rx_opt(&chars, Some((start, end)))];
        for i in 0..rx.ncap {
            groups.push(rx_opt(&chars, caps.get(i).copied().flatten()));
        }
        out.push((chars[last..start].iter().collect(), groups));
        last = end;
        scan = if end > start { end } else { end + 1 };
        if first {
            break;
        }
    }
    (out, chars[last..].iter().collect())
}

// ---- stdlib dispatch ----

pub const REGEX_FNS: &[&str] = &[
    "regex.new",
    "regex.compile",
    "regex.is_match",
    "regex.is_full_match",
    "regex.full_match",
    "regex.find",
    "regex.find_all",
    "regex.index_of",
    "regex.replace",
    "regex.replace_first",
    "regex.replace_with",
    "regex.replace_first_with",
    "regex.split",
    "regex.captures",
    "regex.captures_all",
    "regex.escape",
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

/// a `Regex` value: the record regex.new / regex.compile build
fn rx_value(pat: &str) -> Value {
    Value::Record {
        type_name: Some(Rc::from("Regex")),
        fields: Rc::new(vec![(Rc::from("pattern"), Value::str(pat))]),
    }
}

/// the pattern of a `Regex` argument — re-checked, so a record built by hand
/// aborts like regex.new (C-395)
fn want_regex(name: &str, v: &Value) -> Result<String, Flow> {
    if let Value::Record { fields, .. } = v {
        for (k, val) in fields.iter() {
            if &**k == "pattern" {
                let p = want_str(name, val)?.to_string();
                if let Some(reason) = rx_check(&p) {
                    return Err(Flow::Abort(format!("invalid regex: {reason}")));
                }
                return Ok(p);
            }
        }
    }
    Err(Flow::Fatal(format!(
        "{name}: expected Regex, got {}",
        v.type_name()
    )))
}

pub fn call_regex(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !REGEX_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(it, name, args))
}

fn strs(items: &[String]) -> Value {
    Value::List(Rc::new(items.iter().map(|x| Value::str(x)).collect()))
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        "regex.new" | "regex.compile" => {
            arity(name, &args, 1)?;
            let p = want_str(name, &args[0])?;
            Ok(match (rx_check(p), name == "regex.new") {
                (Some(reason), true) => {
                    return Err(Flow::Abort(format!("invalid regex: {reason}")))
                }
                (Some(reason), false) => Value::Err(Rc::new(Value::str(&reason))),
                (None, true) => rx_value(p),
                (None, false) => Value::Ok(Rc::new(rx_value(p))),
            })
        }
        "regex.escape" => {
            arity(name, &args, 1)?;
            let mut out = String::new();
            for c in want_str(name, &args[0])?.chars() {
                if "\\.+*?()|[]{}^$".contains(c) {
                    out.push('\\');
                }
                out.push(c);
            }
            Ok(Value::str(&out))
        }
        "regex.is_match" | "regex.is_full_match" | "regex.full_match" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            Ok(Value::Bool(if name == "regex.is_match" {
                almide_regex_is_match(&p, s)
            } else {
                almide_regex_full_match(&p, s)
            }))
        }
        "regex.find" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            Ok(match almide_regex_find(&p, s) {
                Some(m) => Value::Some(Rc::new(Value::str(&m))),
                None => Value::None,
            })
        }
        "regex.index_of" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            let rx = rx_compile(&p);
            let chars: Vec<char> = s.chars().collect();
            Ok(match rx_find_at(&rx, &chars, 0) {
                Some((start, _, _)) => Value::Some(Rc::new(Value::Int(start as i64))),
                None => Value::None,
            })
        }
        "regex.find_all" | "regex.split" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            Ok(strs(&if name == "regex.find_all" {
                almide_regex_find_all(&p, s)
            } else {
                almide_regex_split(&p, s)
            }))
        }
        "regex.replace" | "regex.replace_first" => {
            arity(name, &args, 3)?;
            let (p, s, rep) = (
                want_regex(name, &args[0])?,
                want_str(name, &args[1])?,
                want_str(name, &args[2])?,
            );
            Ok(Value::str(&if name == "regex.replace_first" {
                almide_regex_replace_first(&p, s, rep)
            } else {
                almide_regex_replace(&p, s, rep)
            }))
        }
        "regex.replace_with" | "regex.replace_first_with" => {
            arity(name, &args, 3)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            let Value::Fn(f) = &args[2] else {
                return Err(Flow::Fatal(format!("{name}: expected a fn")));
            };
            let f = f.clone();
            let (matches, tail) = rx_scan(&p, s, name == "regex.replace_first_with");
            let mut out = String::new();
            for (field, groups) in matches {
                out.push_str(&field);
                match it.call_value(&f, vec![Value::List(Rc::new(groups))])? {
                    Value::Str(r) => out.push_str(&r),
                    other => {
                        return Err(Flow::Fatal(format!(
                            "{name}: the fn answered {}",
                            other.type_name()
                        )))
                    }
                }
            }
            out.push_str(&tail);
            Ok(Value::str(&out))
        }
        "regex.captures" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            let (matches, _) = rx_scan(&p, s, true);
            Ok(match matches.into_iter().next() {
                Some((_, groups)) => Value::Some(Rc::new(Value::List(Rc::new(groups)))),
                None => Value::None,
            })
        }
        "regex.captures_all" => {
            arity(name, &args, 2)?;
            let (p, s) = (want_regex(name, &args[0])?, want_str(name, &args[1])?);
            let (matches, _) = rx_scan(&p, s, false);
            Ok(Value::List(Rc::new(
                matches
                    .into_iter()
                    .map(|(_, groups)| Value::List(Rc::new(groups)))
                    .collect(),
            )))
        }
        other => Err(Flow::Fatal(format!("unrouted regex fn `{other}`"))),
    }
}
