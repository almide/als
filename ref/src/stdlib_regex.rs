//! Round-3 stdlib module: the ALS-D4 regex engine — transcribed verbatim
//! from the normative oracle `runtime/rs/src/regex.rs` (C-032: one canonical
//! grammar, fuzzed to byte identity across targets by
//! spec/wasm_cross/regex_engine.almd and regex_fuzz_batch.almd). The engine
//! is a backtracking matcher over chars, with the oracle's Pike VM (#4019)
//! taking over in linear time once the walk spends its step budget:
//! alternation with empty arms,
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
    /// the pattern's length in chars (the walk's step budget scales with it)
    size: usize,
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
        size: chars.len(),
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
    /// Steps taken (a goal, or a character a single-atom loop reads) and the
    /// most it may take (usize::MAX = no bound). Past the bound the walk gives
    /// up and `gave_up` says so: the Pike VM answers instead.
    steps: usize,
    budget: usize,
    gave_up: bool,
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
            steps: 0,
            budget: usize::MAX,
            gave_up: false,
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
            self.steps += 1;
            if self.steps > self.budget {
                self.gave_up = true;
                return None;
            }
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
        self.steps += n;
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
            self.steps += 1;
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

/// The text a VM reads and its line-anchor flag.
struct RxCx<'a> {
    s: &'a [char],
    multiline: bool,
}

fn rx_zero_width(cx: &RxCx, node: &RxNode, p: usize) -> Option<bool> {
    match node {
        RxNode::AnchorStart => Some(p == 0 || (cx.multiline && cx.s[p - 1] == '\n')),
        RxNode::AnchorEnd => Some(p == cx.s.len() || (cx.multiline && cx.s[p] == '\n')),
        RxNode::WordBoundary(want) => Some(rx_at_boundary(cx.s, p) == *want),
        _ => None,
    }
}

type RxSeq = Vec<RxPiece>;

// Leftmost match at or after `start` by the backtracking walk: (start, end);
// `vm.caps` holds its groups. None with `vm.gave_up` set: the walk ran out of
// steps before it knew.
fn rx_backtrack_find_at<'a>(
    vm: &mut RxVm<'a>,
    rx: &'a RxPat,
    start: usize,
) -> Option<(usize, usize)> {
    for i in start..=vm.s.len() {
        if let Some(end) = vm.exec(&rx.alts, i) {
            return Some((i, end));
        }
        if vm.gave_up {
            return None;
        }
    }
    None
}

// ---- Linear-time matching: a Pike VM (#4019) ----
//
// The backtracking walk above can take time exponential in the input's length:
// `(a+)+$` on `"aaaa…a!"` tries every way of splitting the `a`s between the two
// loops. The Pike VM gives the SAME answers — the walk's first success, its
// span and every group — in time proportional to the program's size times the
// input's length. The pattern is compiled to a program (Thompson's NFA with
// the walk's preferences as instruction order) and every thread of it is
// advanced one input character at a time, in priority order; two threads that
// reach the same state at the same position have the same future, so only the
// higher-priority one is kept. The walk's first success is the highest-priority
// thread that reaches MATCH, so cutting every lower thread there gives its answer.
//
// What makes a "state" here is (pc, d). The walk has one rule a plain NFA does
// not: an instance of a repeated group that matched nothing counts but never
// loops ("an empty instance counts, but never loops", `resume` above). Whether
// an instance is empty depends on where it began, so two threads at one pc can
// differ in their futures. Only instances that began at the CURRENT position can
// end empty, and instances nest, so the open instances that began here are
// always the innermost ones: `d` counts the open instances (of groups whose body
// can match empty — the "tracked" ones) that began BEFORE here, and the rest
// began here. Reading a character makes every open instance old.
//
// Counted repetition is expanded when the program stays small: `x{2,4}`
// becomes x x (x (x)?)?, one copy of the body per count, so the count is part
// of pc. Past `RX_PIKE_STATES` states (`.{0,20000}`, `((a){200}){200}`)
// the pattern is compiled COUNTED instead (#4083): each repetition with more
// than one copy is ONE copy of its body between a CLoop and a CEnd, and the
// count lives in a per-thread counter register — one register per nesting
// level of counted repetitions, 0 whenever the thread is outside the loop
// (CReset puts it back on the way out). A state is then (pc, d, registers).
// That is exactly the expanded program's state, written differently: the
// expanded pc IS (pc, the counts of the loops around it), the instructions are
// in the same order, and so the threads, their priorities and every answer are
// the same; only the program stops growing with the counts. Two threads in
// one state are still one, found through a hash table keyed by the whole state
// instead of an array indexed by (pc, d). The cost per character is the number
// of states threads are in, at most the expanded program's state count, so a
// match still takes time proportional to that count times the input's length —
// the bound the expansion had — with memory for the live threads only. The
// self-hosted twin in stdlib/regex_engine.almd compiles the same programs and
// runs the same VM, so the two legs still agree on every match, capture and
// failure. Since #3942 the public API refuses a pattern past
// RX_MAX_STATES, which is this budget, so an accepted pattern always
// runs expanded; the counted program is what the VM runs if that limit is
// raised, and tests/regex_pike_differential_test.rs runs it on every pattern.

/// The most (pc, d) states an expanded program may have before the pattern is
/// compiled counted instead. Every ordinary pattern is far below it; what
/// reaches it is a counted repetition in the thousands (`.{0,10000}`) or nested
/// counts multiplying (`((a){100}){100}`).
const RX_PIKE_STATES: usize = 20000;

/// A target that is never taken: the empty instance of a repetition that has
/// not reached its minimum fails.
const RX_NOWHERE: usize = usize::MAX;

#[derive(Clone, Copy)]
enum RxInst {
    /// Read one character the atom accepts.
    Char(usize),
    /// A zero-width test (`^`, `$`, `\b`, `\B`).
    Assert(usize),
    /// Go on at both, the first preferred.
    Split(usize, usize),
    Jmp(usize),
    /// An instance of capturing group `ci` (1-based) begins here.
    GStart(usize),
    /// An instance of a group ends: record capture `ci` (0 = none), then go on
    /// at `nonempty`, or at `empty` when the instance matched nothing (only a
    /// tracked group, whose `outer` is the d count outside it, can tell).
    GEnd {
        ci: usize,
        tracked: bool,
        outer: usize,
        empty: usize,
        nonempty: usize,
    },
    /// Counted mode: the decision before another instance of a repetition
    /// whose count is register `level` (c): past `max` leave for `exit`; from
    /// `min` on greedy prefers another instance (pc + 1), lazy prefers `exit`;
    /// below `min` the instance is required.
    CLoop {
        level: usize,
        min: usize,
        max: Option<usize>,
        lazy: bool,
        exit: usize,
    },
    /// Counted mode: an instance ends. Record capture `ci` like GEnd; an empty
    /// instance (tracked groups only) leaves for pc + 1 (or fails below
    /// `min`), any other counts one more and goes back to the CLoop at `lp`.
    CEnd {
        ci: usize,
        tracked: bool,
        outer: usize,
        lp: usize,
    },
    /// Counted mode: leave a repetition, its register back to 0.
    CReset(usize),
    Match,
    Fail,
}

struct RxProg {
    insts: Vec<RxInst>,
    /// The atoms the Char and Assert instructions name.
    nodes: Vec<RxNode>,
    /// The number of tracked group instances open at each pc.
    depth: Vec<usize>,
    /// States per pc: one more than the deepest nesting of tracked groups.
    width: usize,
    /// Counter registers (counted mode): the deepest nesting of counted
    /// repetitions; 0 for an expanded program.
    regs: usize,
    /// The atoms a match can begin with, when every match begins by reading
    /// one (no assertion or empty match before the first character). A start
    /// position whose character none of them accepts cannot begin a match, so
    /// the search skips it rather than starting threads that die at once.
    first: Option<Vec<usize>>,
}

/// The atoms read first from pc 0 — None when an assertion or MATCH can come
/// before any of them.
fn rx_first_atoms(insts: &[RxInst]) -> Option<Vec<usize>> {
    let mut seen = vec![false; insts.len()];
    let mut todo = vec![0usize];
    let mut atoms: Vec<usize> = Vec::new();
    while let Some(pc) = todo.pop() {
        if pc >= insts.len() || seen[pc] {
            continue;
        }
        seen[pc] = true;
        match insts[pc] {
            RxInst::Char(node) => atoms.push(node),
            RxInst::Assert(_) | RxInst::Match => return None,
            RxInst::Split(a, b) => todo.extend([a, b]),
            RxInst::Jmp(a) => todo.push(a),
            RxInst::GStart(_) => todo.push(pc + 1),
            RxInst::GEnd {
                empty, nonempty, ..
            } => todo.extend([empty, nonempty]),
            // Before any character every register is 0, so a loop with a
            // minimum is entered; an instance can only end empty here.
            RxInst::CLoop { min, exit, .. } => {
                todo.push(pc + 1);
                if min == 0 {
                    todo.push(exit);
                }
            }
            RxInst::CEnd { .. } | RxInst::CReset(_) => todo.push(pc + 1),
            RxInst::Fail => {}
        }
    }
    Some(atoms)
}

struct RxComp<'a> {
    insts: Vec<RxInst>,
    nodes: Vec<RxNode>,
    /// Each AST atom's index in `nodes`, so its copies share one entry.
    index: std::collections::BTreeMap<usize, usize>,
    pat: std::marker::PhantomData<&'a RxPat>,
    depth: Vec<usize>,
    cur: usize,
    deepest: usize,
    over: bool,
    /// Compile repetitions counted (one body copy, a counter register).
    counted: bool,
    /// Counted repetitions open here, and the most ever open.
    level: usize,
    levels: usize,
}

/// Can `alts` match without reading a character? Assertions count as
/// matching nothing, so this over-approximates, which is the safe side: a group
/// wrongly called nullable only carries a little more state.
fn rx_nullable(alts: &[RxSeq]) -> bool {
    alts.iter().any(|seq| {
        seq.iter().all(|piece| {
            piece.min == 0
                || match &piece.node {
                    RxNode::AnchorStart | RxNode::AnchorEnd | RxNode::WordBoundary(_) => true,
                    RxNode::Group(inner, _) => rx_nullable(inner),
                    _ => false,
                }
        })
    })
}

impl<'a> RxComp<'a> {
    fn node(&mut self, node: &'a RxNode) -> usize {
        let next = self.nodes.len();
        let i = *self
            .index
            .entry(node as *const RxNode as usize)
            .or_insert(next);
        if i == next {
            self.nodes.push(node.clone());
        }
        i
    }

    fn emit(&mut self, inst: RxInst) -> usize {
        self.insts.push(inst);
        self.depth.push(self.cur);
        if !self.counted && self.insts.len() * (self.deepest + 1) > RX_PIKE_STATES {
            self.over = true;
        }
        self.insts.len() - 1
    }

    fn here(&self) -> usize {
        self.insts.len()
    }

    /// Point every placeholder target in `at` (a Split's second arm, a Jmp, or
    /// a GEnd's empty / nonempty arm, marked usize::MAX - 1) at `to`.
    fn patch(&mut self, at: &[usize], to: usize) {
        const HOLE: usize = usize::MAX - 1;
        for &i in at {
            self.insts[i] = match self.insts[i] {
                RxInst::Split(a, b) => RxInst::Split(
                    if a == HOLE { to } else { a },
                    if b == HOLE { to } else { b },
                ),
                RxInst::Jmp(_) => RxInst::Jmp(to),
                RxInst::GEnd {
                    ci,
                    tracked,
                    outer,
                    empty,
                    nonempty,
                } => RxInst::GEnd {
                    ci,
                    tracked,
                    outer,
                    empty: if empty == HOLE { to } else { empty },
                    nonempty: if nonempty == HOLE { to } else { nonempty },
                },
                RxInst::CLoop {
                    level,
                    min,
                    max,
                    lazy,
                    ..
                } => RxInst::CLoop {
                    level,
                    min,
                    max,
                    lazy,
                    exit: to,
                },
                other => other,
            };
        }
    }

    /// The alternatives in order, each one's end jumping past the rest.
    fn alts(&mut self, alts: &'a [RxSeq]) {
        const HOLE: usize = usize::MAX - 1;
        let mut ends = Vec::new();
        for (i, seq) in alts.iter().enumerate() {
            if self.over {
                return;
            }
            if i + 1 < alts.len() {
                let split = self.emit(RxInst::Split(self.here() + 1, HOLE));
                self.seq(seq);
                ends.push(self.emit(RxInst::Jmp(HOLE)));
                let next = self.here();
                self.patch(&[split], next);
            } else {
                self.seq(seq);
            }
        }
        let end = self.here();
        self.patch(&ends, end);
    }

    fn seq(&mut self, seq: &'a [RxPiece]) {
        for piece in seq {
            if self.over {
                return;
            }
            match &piece.node {
                // A quantifier on a zero-width piece is parsed and ignored.
                RxNode::AnchorStart | RxNode::AnchorEnd | RxNode::WordBoundary(_) => {
                    let n = self.node(&piece.node);
                    self.emit(RxInst::Assert(n));
                }
                RxNode::Group(alts, ci) => self.group(piece, alts, *ci),
                node => self.atom(piece, node),
            }
        }
    }

    /// A single-character atom: `min` copies, then the optional ones —
    /// greedy prefers another character, lazy prefers to stop.
    fn atom(&mut self, piece: &'a RxPiece, node: &'a RxNode) {
        const HOLE: usize = usize::MAX - 1;
        if piece.max.is_some_and(|mx| piece.min > mx) {
            self.emit(RxInst::Fail);
            return;
        }
        let n = self.node(node);
        if self.counted && rx_copies(piece.min, piece.max) > 1 {
            self.counted_loop(piece.min, piece.max, piece.lazy, false, 0, |comp| {
                comp.emit(RxInst::Char(n));
            });
            return;
        }
        for _ in 0..piece.min {
            if self.over {
                return;
            }
            self.emit(RxInst::Char(n));
        }
        let mut exits = Vec::new();
        match piece.max {
            None => {
                let top = self.here();
                let split = self.emit(RxInst::Split(HOLE, HOLE));
                let body = self.emit(RxInst::Char(n));
                self.emit(RxInst::Jmp(top));
                let exit = self.here();
                self.insts[split] = if piece.lazy {
                    RxInst::Split(exit, body)
                } else {
                    RxInst::Split(body, exit)
                };
            }
            Some(mx) => {
                for _ in piece.min..mx {
                    if self.over {
                        return;
                    }
                    let split = self.emit(RxInst::Split(HOLE, HOLE));
                    let body = self.emit(RxInst::Char(n));
                    self.insts[split] = if piece.lazy {
                        RxInst::Split(HOLE, body)
                    } else {
                        RxInst::Split(body, HOLE)
                    };
                    exits.push(split);
                }
                let exit = self.here();
                self.patch(&exits, exit);
            }
        }
    }

    /// A group with its repetition, one copy of the body per count the walk
    /// keeps apart: R_c (the decision after c instances) then instance c.
    /// Below `min` an instance is required; at or above it greedy prefers one
    /// more, lazy prefers to stop; an unbounded repetition's last copy loops.
    fn group(&mut self, piece: &'a RxPiece, alts: &'a [RxSeq], ci: usize) {
        const HOLE: usize = usize::MAX - 1;
        let (min, max, lazy) = (piece.min, piece.max, piece.lazy);
        if max.is_some_and(|mx| min > mx) {
            self.emit(RxInst::Fail);
            return;
        }
        let tracked = (min, max) != (1, Some(1)) && rx_nullable(alts);
        if self.counted && rx_copies(min, max) > 1 {
            self.counted_loop(min, max, lazy, tracked, ci, |comp| {
                if ci > 0 {
                    comp.emit(RxInst::GStart(ci));
                }
                comp.alts(alts);
            });
            return;
        }
        let outer = self.cur;
        if tracked && outer + 1 > self.deepest {
            self.deepest = outer + 1;
        }
        // The last count with its own copy: an unbounded repetition loops at
        // `min`, a bounded one stops at `max`.
        let last = max.unwrap_or(min);
        let mut exits = Vec::new();
        let mut c = 0;
        loop {
            if self.over {
                return;
            }
            let more = max.is_none_or(|mx| c < mx);
            if !more {
                break; // R_max: stop, which is the exit itself
            }
            let r_c = self.here();
            if c >= min {
                let split = if lazy {
                    self.emit(RxInst::Split(HOLE, r_c + 1))
                } else {
                    self.emit(RxInst::Split(r_c + 1, HOLE))
                };
                exits.push(split);
            }
            // instance c
            if tracked {
                self.cur += 1;
            }
            if ci > 0 {
                self.emit(RxInst::GStart(ci));
            }
            self.alts(alts);
            let loops = max.is_none() && c == min;
            let empty = if c + 1 < min { RX_NOWHERE } else { HOLE };
            let nonempty = if loops { r_c } else { self.here() + 1 };
            let end = self.emit(RxInst::GEnd {
                ci,
                tracked,
                outer,
                empty,
                nonempty,
            });
            if tracked {
                self.cur -= 1;
            }
            exits.push(end);
            if loops || c >= last {
                break;
            }
            c += 1;
        }
        let exit = self.here();
        self.patch(&exits, exit);
    }

    /// Counted mode: the repetition as CLoop, the body (`body` emits it: a
    /// Char, or a group's GStart and alternatives), CEnd, CReset — the
    /// expanded copies folded into one, the count in register `level`.
    fn counted_loop(
        &mut self,
        min: usize,
        max: Option<usize>,
        lazy: bool,
        tracked: bool,
        ci: usize,
        body: impl FnOnce(&mut Self),
    ) {
        const HOLE: usize = usize::MAX - 1;
        let level = self.level;
        self.level += 1;
        self.levels = self.levels.max(self.level);
        let outer = self.cur;
        if tracked && outer + 1 > self.deepest {
            self.deepest = outer + 1;
        }
        let lp = self.emit(RxInst::CLoop {
            level,
            min,
            max,
            lazy,
            exit: HOLE,
        });
        if tracked {
            self.cur += 1;
        }
        body(self);
        self.emit(RxInst::CEnd {
            ci,
            tracked,
            outer,
            lp,
        });
        if tracked {
            self.cur -= 1;
        }
        self.level -= 1;
        let exit = self.emit(RxInst::CReset(level));
        self.patch(&[lp], exit);
    }
}

/// How many copies of its body a repetition expands to: one per count up to
/// `max`, or up to `min` and then one that loops.
fn rx_copies(min: usize, max: Option<usize>) -> usize {
    max.unwrap_or(min.saturating_add(1))
}

/// The pattern's program: expanded while that fits in RX_PIKE_STATES
/// states, counted past it (see the comment above RX_PIKE_STATES).
/// None only for a pattern over the walk's own use (never).
fn rx_program(rx: &RxPat) -> Option<RxProg> {
    rx_program_as(rx, false).or_else(|| rx_program_as(rx, true))
}

/// The program compiled expanded (None past the state budget) or counted.
fn rx_program_as(rx: &RxPat, counted: bool) -> Option<RxProg> {
    let mut comp = RxComp {
        insts: Vec::new(),
        nodes: Vec::new(),
        index: std::collections::BTreeMap::new(),
        pat: std::marker::PhantomData,
        depth: Vec::new(),
        cur: 0,
        deepest: 0,
        over: false,
        counted,
        level: 0,
        levels: 0,
    };
    comp.alts(&rx.alts);
    comp.emit(RxInst::Match);
    if comp.over {
        return None;
    }
    let first = rx_first_atoms(&comp.insts);
    Some(RxProg {
        insts: comp.insts,
        nodes: comp.nodes,
        depth: comp.depth,
        width: comp.deepest + 1,
        regs: comp.levels,
        first,
    })
}

/// One list of threads, in priority order: each at a pc with its d, and its
/// `stride` slots ([match start, then per group: start, end, instance start,
/// then the counter registers]).
struct RxThreads {
    pcs: Vec<(usize, usize)>,
    slots: Vec<usize>,
    /// The stamp each state was last added under; a state is in this list when
    /// its stamp is the list's. Indexed by (pc, d) for an expanded program.
    seen: Vec<u64>,
    stamp: u64,
    /// Counted mode: the states added under the current stamp, an open-
    /// addressing hash table of entries [stamp, pc, d, registers...]; an entry
    /// with an older stamp is free.
    table: Vec<u64>,
    /// Entries in `table` (a power of two, 0 before the first), and used.
    cap: usize,
    used: usize,
}

impl RxThreads {
    fn new(states: usize) -> Self {
        RxThreads {
            pcs: Vec::new(),
            slots: Vec::new(),
            seen: vec![0; states],
            stamp: 0,
            table: Vec::new(),
            cap: 0,
            used: 0,
        }
    }

    fn clear(&mut self) {
        self.pcs.clear();
        self.slots.clear();
        self.stamp += 1;
        self.used = 0;
    }

    /// Counted mode: add the state (pc, d, regs) under the current stamp —
    /// false when it is already there. The self-hosted twin hashes the same way.
    fn visit(&mut self, pc: usize, d: usize, regs: &[usize]) -> bool {
        let entry = 3 + regs.len();
        if (self.used + 1) * 2 > self.cap {
            self.grow(entry);
        }
        let cap = self.cap;
        let mut i = rx_hash(pc, d, regs, cap - 1);
        loop {
            let at = i * entry;
            if self.table[at] != self.stamp {
                self.table[at] = self.stamp;
                self.table[at + 1] = pc as u64;
                self.table[at + 2] = d as u64;
                for (k, &r) in regs.iter().enumerate() {
                    self.table[at + 3 + k] = r as u64;
                }
                self.used += 1;
                return true;
            }
            if self.table[at + 1] == pc as u64
                && self.table[at + 2] == d as u64
                && regs
                    .iter()
                    .enumerate()
                    .all(|(k, &r)| self.table[at + 3 + k] == r as u64)
            {
                return false;
            }
            i = (i + 1) & (cap - 1);
        }
    }

    /// Twice the entries (1024 at first), the current ones moved over.
    fn grow(&mut self, entry: usize) {
        let old_cap = self.cap;
        let cap = if old_cap == 0 { 1024 } else { old_cap * 2 };
        let old = std::mem::replace(&mut self.table, vec![0; cap * entry]);
        self.cap = cap;
        // Stamps start at 1, so a zeroed entry is free.
        for e in 0..old_cap {
            let at = e * entry;
            if old[at] != self.stamp {
                continue;
            }
            let regs: Vec<usize> = old[at + 3..at + entry]
                .iter()
                .map(|&r| r as usize)
                .collect();
            let mut i = rx_hash(old[at + 1] as usize, old[at + 2] as usize, &regs, cap - 1);
            while self.table[i * entry] == self.stamp {
                i = (i + 1) & (cap - 1);
            }
            self.table[i * entry..(i + 1) * entry].copy_from_slice(&old[at..at + entry]);
        }
    }
}

/// A state's bucket in a table of `mask + 1` (a power of two) entries. Every
/// step stays below 2^56, so the self-hosted twin computes it exactly.
fn rx_hash(pc: usize, d: usize, regs: &[usize], mask: usize) -> usize {
    let mut h = pc & mask;
    h = (h * 40503 + (d & mask)) & mask;
    for &r in regs {
        h = (h * 40503 + (r & mask)) & mask;
    }
    // Spread runs of neighbouring keys (one pc, consecutive counts) apart, so
    // linear probing does not walk them.
    (h * 40503) & mask
}

enum RxStep {
    Go(usize, usize),
    Restore(usize, usize),
}

/// The slot that holds no position.
const RX_UNSET: usize = usize::MAX;

/// The VM's working memory, kept across the searches of one call (find_all,
/// replace and split search once per match).
struct RxPike {
    clist: RxThreads,
    nlist: RxThreads,
    stack: Vec<RxStep>,
    scratch: Vec<usize>,
    /// The slots of the match found (slot 0 = its start).
    found: Vec<usize>,
    /// Only `captures` reads the groups, so only it carries them.
    track: bool,
    /// Slots per thread: the capture slots (`ncap`), then `regs` counter
    /// registers.
    nslots: usize,
    ncap_slots: usize,
    regs: usize,
}

impl RxPike {
    fn new(prog: &RxProg, ncap: usize, track: bool) -> Self {
        // A counted program finds its states through the hash table.
        let states = if prog.regs > 0 {
            0
        } else {
            prog.insts.len() * prog.width
        };
        let ncap_slots = if track { 1 + 3 * ncap } else { 1 };
        let nslots = ncap_slots + prog.regs;
        RxPike {
            clist: RxThreads::new(states),
            nlist: RxThreads::new(states),
            stack: Vec::new(),
            scratch: vec![RX_UNSET; nslots],
            found: vec![RX_UNSET; nslots],
            track,
            nslots,
            ncap_slots,
            regs: prog.regs,
        }
    }

    /// Add the thread at (pc, d) at text position `p`, and everything it
    /// reaches without reading, to the next list (`next`) or the current one,
    /// in priority order. The scratch slots are the thread's; the closure
    /// changes them and puts them back.
    fn add(&mut self, prog: &RxProg, cx: &RxCx, next: bool, pc: usize, d: usize, p: usize) {
        let w = prog.width;
        let list = if next {
            &mut self.nlist
        } else {
            &mut self.clist
        };
        let slots = &mut self.scratch;
        let stack = &mut self.stack;
        stack.push(RxStep::Go(pc, d));
        while let Some(step) = stack.pop() {
            let (pc, d) = match step {
                RxStep::Restore(slot, v) => {
                    slots[slot] = v;
                    continue;
                }
                RxStep::Go(pc, d) => (pc, d),
            };
            let r0 = self.ncap_slots;
            if self.regs > 0 {
                // A CEnd, CReset or GStart is not looked up: no cycle runs
                // through one without a CLoop or Split, which are, and a
                // second visit only reaches states already there.
                let pass = matches!(
                    prog.insts[pc],
                    RxInst::CEnd { .. } | RxInst::CReset(_) | RxInst::GStart(_)
                );
                if !pass && !list.visit(pc, d, &slots[r0..r0 + self.regs]) {
                    continue;
                }
            } else {
                let key = pc * w + d;
                if list.seen[key] == list.stamp {
                    continue;
                }
                list.seen[key] = list.stamp;
            }
            match prog.insts[pc] {
                RxInst::Char(_) | RxInst::Match => {
                    list.pcs.push((pc, d));
                    list.slots.extend_from_slice(slots);
                }
                RxInst::Assert(node) => {
                    if rx_zero_width(cx, &prog.nodes[node], p) == Some(true) {
                        stack.push(RxStep::Go(pc + 1, d));
                    }
                }
                RxInst::Split(a, b) => {
                    stack.push(RxStep::Go(b, d));
                    stack.push(RxStep::Go(a, d));
                }
                RxInst::Jmp(a) => stack.push(RxStep::Go(a, d)),
                RxInst::GStart(ci) => {
                    if self.track {
                        let slot = 3 * ci;
                        stack.push(RxStep::Restore(slot, slots[slot]));
                        slots[slot] = p;
                    }
                    stack.push(RxStep::Go(pc + 1, d));
                }
                RxInst::GEnd {
                    ci,
                    tracked,
                    outer,
                    empty,
                    nonempty,
                } => {
                    if ci > 0 && self.track {
                        let (s0, s1) = (3 * ci - 2, 3 * ci - 1);
                        stack.push(RxStep::Restore(s0, slots[s0]));
                        stack.push(RxStep::Restore(s1, slots[s1]));
                        slots[s0] = slots[3 * ci];
                        slots[s1] = p;
                    }
                    // The instance began here when it is not among the `d`
                    // instances that began before.
                    let is_empty = tracked && d <= outer;
                    let to = if is_empty { empty } else { nonempty };
                    if to != RX_NOWHERE {
                        stack.push(RxStep::Go(to, d.min(outer)));
                    }
                }
                RxInst::CLoop {
                    level,
                    min,
                    max,
                    lazy,
                    exit,
                } => {
                    let c = slots[r0 + level];
                    if max.is_some_and(|mx| c >= mx) {
                        stack.push(RxStep::Go(exit, d));
                    } else if c >= min {
                        let (first, second) = if lazy { (exit, pc + 1) } else { (pc + 1, exit) };
                        stack.push(RxStep::Go(second, d));
                        stack.push(RxStep::Go(first, d));
                    } else {
                        stack.push(RxStep::Go(pc + 1, d));
                    }
                }
                RxInst::CEnd {
                    ci,
                    tracked,
                    outer,
                    lp,
                } => {
                    if ci > 0 && self.track {
                        let (s0, s1) = (3 * ci - 2, 3 * ci - 1);
                        stack.push(RxStep::Restore(s0, slots[s0]));
                        stack.push(RxStep::Restore(s1, slots[s1]));
                        slots[s0] = slots[3 * ci];
                        slots[s1] = p;
                    }
                    let RxInst::CLoop {
                        level, min, max, ..
                    } = prog.insts[lp]
                    else {
                        continue;
                    };
                    let reg = r0 + level;
                    let c = slots[reg];
                    if tracked && d <= outer {
                        // empty: it counts but ends the repetition
                        if c + 1 >= min {
                            stack.push(RxStep::Go(pc + 1, d.min(outer)));
                        }
                    } else {
                        stack.push(RxStep::Restore(reg, c));
                        slots[reg] = if max.is_none() {
                            (c + 1).min(min)
                        } else {
                            c + 1
                        };
                        stack.push(RxStep::Go(lp, d.min(outer)));
                    }
                }
                RxInst::CReset(level) => {
                    let reg = r0 + level;
                    stack.push(RxStep::Restore(reg, slots[reg]));
                    slots[reg] = 0;
                    stack.push(RxStep::Go(pc + 1, d));
                }
                RxInst::Fail => {}
            }
        }
    }

    /// Start a thread for a match beginning at `p` in the current list (`next`
    /// false) or the next one.
    fn seed(&mut self, prog: &RxProg, cx: &RxCx, next: bool, p: usize) {
        self.scratch.iter_mut().for_each(|s| *s = RX_UNSET);
        self.scratch[self.ncap_slots..]
            .iter_mut()
            .for_each(|r| *r = 0);
        self.scratch[0] = p;
        self.add(prog, cx, next, 0, 0, p);
    }

    /// The first position at or after `p` where a match can begin by reading
    /// one of `first`; None when there is none.
    fn skip(prog: &RxProg, cx: &RxCx, first: &[usize], p: usize) -> Option<usize> {
        (p..cx.s.len()).find(|&q| {
            first
                .iter()
                .any(|&node| rx_node_matches(&prog.nodes[node], cx.s[q]))
        })
    }

    /// The walk's first success from a start at or after `start` (only at
    /// `start` when `anchored`), ending at `end_at` when that is given: its
    /// end, with its slots in `found`. With `any`, the first match found,
    /// whichever it is.
    fn search(
        &mut self,
        prog: &RxProg,
        cx: &RxCx,
        start: usize,
        anchored: bool,
        end_at: Option<usize>,
        any: bool,
    ) -> Option<usize> {
        let mut found: Option<usize> = None;
        let len = cx.s.len();
        let first = if anchored {
            None
        } else {
            prog.first.as_deref()
        };
        let mut p = start;
        if let Some(first) = first {
            p = Self::skip(prog, cx, first, p)?;
        }
        self.clist.clear();
        self.seed(prog, cx, false, p);
        loop {
            self.nlist.clear();
            let c = cx.s.get(p).copied();
            for i in 0..self.clist.pcs.len() {
                let (pc, _) = self.clist.pcs[i];
                let at = i * self.nslots;
                match prog.insts[pc] {
                    RxInst::Match if end_at.is_none_or(|e| e == p) => {
                        self.found
                            .copy_from_slice(&self.clist.slots[at..at + self.nslots]);
                        found = Some(p);
                        if any {
                            return found;
                        }
                        // Every thread after this one has lower priority.
                        break;
                    }
                    RxInst::Char(node)
                        if c.is_some_and(|c| rx_node_matches(&prog.nodes[node], c)) =>
                    {
                        self.scratch
                            .copy_from_slice(&self.clist.slots[at..at + self.nslots]);
                        self.add(prog, cx, true, pc + 1, prog.depth[pc], p + 1);
                    }
                    _ => {}
                }
            }
            if p >= len {
                break;
            }
            if found.is_none() && !anchored {
                // A match starting further right has the lowest priority.
                match first {
                    // With nothing in flight, go straight to the next place a
                    // match can begin.
                    Some(first) if self.nlist.pcs.is_empty() => {
                        let Some(q) = Self::skip(prog, cx, first, p + 1) else {
                            break;
                        };
                        self.clist.clear();
                        self.seed(prog, cx, false, q);
                        p = q;
                        continue;
                    }
                    _ => self.seed(prog, cx, true, p + 1),
                }
            }
            if self.nlist.pcs.is_empty() && (found.is_some() || anchored) {
                break;
            }
            std::mem::swap(&mut self.clist, &mut self.nlist);
            p += 1;
        }
        found
    }
}

/// A compiled pattern: its tree (the walk's) and its program (the VM's).
struct RxCompiled {
    rx: RxPat,
    prog: Option<RxProg>,
}

fn rx_compiled(pat: &str) -> RxCompiled {
    let rx = rx_compile(pat);
    let prog = rx_program(&rx);
    RxCompiled { rx, prog }
}

/// How many walk steps per (pattern char × text char) a call may spend before
/// the Pike VM takes over. An ordinary pattern needs a few steps per text
/// char, so it never comes near the bound; one that would backtrack without
/// end reaches it after time linear in the text, and the VM finishes the job.
const RX_WALK_STEPS: usize = 4;

/// A compiled pattern over one text. The backtracking walk runs first, under
/// a step budget linear in the text's length: it is the faster engine on the
/// patterns people write, and it gives the same answers as the Pike VM. When
/// the budget runs out the VM answers instead, so a call never takes more than
/// linear time. Every pattern has a program (#4083), so no call keeps the
/// walk past its budget.
struct RxMatcher<'a> {
    c: &'a RxCompiled,
    track: bool,
    /// The VM's working memory, once the walk has given up.
    pike: Option<RxPike>,
    vm: RxVm<'a>,
}

impl<'a> RxMatcher<'a> {
    fn new(c: &'a RxCompiled, s: &'a [char], track: bool) -> Self {
        let mut vm = RxVm::new(&c.rx, s, None);
        if c.prog.is_some() {
            vm.budget = RX_WALK_STEPS
                .saturating_mul(c.rx.size + 1)
                .saturating_mul(s.len() + 1);
        }
        RxMatcher {
            c,
            track,
            pike: None,
            vm,
        }
    }

    /// The VM, from the pattern's kept working memory or new — None when the
    /// walk still has steps left (or the pattern has no program).
    fn pike(&mut self) -> Option<(&RxProg, &mut RxPike)> {
        if !self.vm.gave_up {
            return None;
        }
        let prog = self.c.prog.as_ref()?;
        if self.pike.is_none() {
            self.pike = Some(RxPike::new(prog, self.c.rx.ncap, self.track));
        }
        Some((prog, self.pike.as_mut()?))
    }

    /// Leftmost match at or after `start`: (start, end); `caps` then holds
    /// its groups (when tracked).
    fn find_at(&mut self, start: usize) -> Option<(usize, usize)> {
        if !self.vm.gave_up {
            let found = rx_backtrack_find_at(&mut self.vm, &self.c.rx, start);
            if !self.vm.gave_up {
                return found;
            }
        }
        let cx = RxCx {
            s: self.vm.s,
            multiline: self.vm.multiline,
        };
        let (prog, pike) = self.pike()?;
        let end = pike.search(prog, &cx, start, false, None, false)?;
        let (first, track) = (pike.found[0], pike.track);
        if track {
            let found = pike.found.clone();
            for (i, cap) in self.vm.caps.iter_mut().enumerate() {
                let (s0, s1) = (found[3 * i + 1], found[3 * i + 2]);
                *cap = if s0 == RX_UNSET { None } else { Some((s0, s1)) };
            }
        }
        Some((first, end))
    }

    fn is_match(&mut self) -> bool {
        self.find_at(0).is_some()
    }

    fn full_match(&mut self) -> bool {
        let len = self.vm.s.len();
        if !self.vm.gave_up {
            self.vm.end_at = Some(len);
            let matched = self.vm.exec(&self.c.rx.alts, 0).is_some();
            if !self.vm.gave_up {
                return matched;
            }
        }
        let cx = RxCx {
            s: self.vm.s,
            multiline: self.vm.multiline,
        };
        match self.pike() {
            Some((prog, pike)) => pike.search(prog, &cx, 0, true, Some(len), false).is_some(),
            None => false,
        }
    }

    fn caps(&self) -> &RxCaps {
        &self.vm.caps
    }
}

// ---- Public API ----

pub fn almide_regex_is_match(pat: &str, s: &str) -> bool {
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    RxMatcher::new(&rx, &chars, false).is_match()
}

pub fn almide_regex_full_match(pat: &str, s: &str) -> bool {
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    RxMatcher::new(&rx, &chars, false).full_match()
}

pub fn almide_regex_find(pat: &str, s: &str) -> Option<String> {
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, false);
    m.find_at(0)
        .map(|(start, end)| chars[start..end].iter().collect())
}

pub fn almide_regex_find_all(pat: &str, s: &str) -> Vec<String> {
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, false);
    let mut results: Vec<String> = vec![];
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some((start, end)) = m.find_at(pos) {
            results.push(chars[start..end].iter().collect());
            pos = if end > start { end } else { end + 1 };
        } else {
            break;
        }
    }
    results
}

pub fn almide_regex_replace(pat: &str, s: &str, rep: &str) -> String {
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, false);
    let mut result = String::new();
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some((start, end)) = m.find_at(pos) {
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
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, false);
    if let Some((start, end)) = m.find_at(0) {
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
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, false);
    let mut results: Vec<String> = vec![];
    let (mut last, mut scan) = (0usize, 0usize);
    while scan <= chars.len() {
        let Some((start, end)) = m.find_at(scan) else {
            break;
        };
        results.push(chars[last..start].iter().collect());
        last = end;
        scan = if end > start { end } else { end + 1 };
    }
    results.push(chars[last..].iter().collect());
    results
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
    let rx = rx_compiled(pat);
    let chars: Vec<char> = s.chars().collect();
    let mut m = RxMatcher::new(&rx, &chars, true);
    let mut out = Vec::new();
    let (mut last, mut scan) = (0usize, 0usize);
    while scan <= chars.len() {
        let Some((start, end)) = m.find_at(scan) else {
            break;
        };
        let caps = m.caps().clone();
        let mut groups = vec![rx_opt(&chars, Some((start, end)))];
        for i in 0..rx.rx.ncap {
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
            let rx = rx_compiled(&p);
            let chars: Vec<char> = s.chars().collect();
            Ok(match RxMatcher::new(&rx, &chars, false).find_at(0) {
                Some((start, _)) => Value::Some(Rc::new(Value::Int(start as i64))),
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
