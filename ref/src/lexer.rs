//! Lexer — written from the ALS grammar (the EBNF in the implementation's
//! `docs/GRAMMAR.md` §Literals/§Notes, ALS-E1/E2/E5/E16). No compiler crate is
//! consulted; where the grammar text and the accepted corpus disagree, the
//! corpus wins and the divergence is recorded in `docs/ref/PARSER-NOTES.md`.
//!
//! Tokens carry `line` (1-based) for diagnostics and `spaced` — whether
//! whitespace (or a newline) preceded the token — which the parser needs for
//! the one whitespace-sensitive rule of the language: a line-initial `-`
//! glued to its operand starts a new statement, an unglued one continues the
//! previous expression.

#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Text(String),
    /// Source text of a `${ … }` interpolation, parsed later by the parser
    /// (so interpolations may nest strings and braces freely).
    Interp(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// lowercase-initial or `_`-initial name, or a keyword (the parser decides)
    Ident(String),
    /// backtick-escaped identifier: never a keyword
    EscIdent(String),
    /// uppercase-initial name
    TypeName(String),
    Int(i64),
    /// the literal's digits, kept textual: the evaluator parses floats itself
    /// (ADR-0015 clause 5 — `str::parse` is forbidden here)
    Float(String),
    /// double-quoted / heredoc string with interpolation parts
    Str(Vec<StrPart>),
    /// single-quoted or raw string: literal text only
    PlainStr(String),
    Sym(&'static str),
    Newline,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
    pub spaced: bool,
}

#[derive(Debug)]
pub struct LexError {
    pub line: usize,
    pub msg: String,
}

const SYMS: &[&str] = &[
    "..<", "...", "?.", "??", "|>", ">>", "=>", "->", "==", "!=", "<=", ">=", "**", "..",
    "+", "-", "*", "/", "%", "^", "<", ">", "=", "!", "?", ".", ",", ":", ";", "(", ")", "[", "]",
    "{", "}", "|", "@",
];

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut spaced = true;
    let n = chars.len();
    while i < n {
        let c = chars[i];
        // whitespace (not newline)
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            spaced = true;
            continue;
        }
        if c == '\n' {
            if !matches!(out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
                out.push(Token { tok: Tok::Newline, line, col: i - line_start + 1, spaced: true });
            }
            i += 1;
            line += 1;
            line_start = i;
            spaced = true;
            continue;
        }
        // comments
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            let start_line = line;
            let mut depth = 0usize;
            while i < n {
                if chars[i] == '/' && i + 1 < n && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                        line_start = i + 1;
                    }
                    i += 1;
                }
            }
            if depth != 0 {
                return Err(LexError { line: start_line, msg: "unterminated block comment".into() });
            }
            spaced = true;
            continue;
        }
        let col = i - line_start + 1;
        // raw strings: r"..." / r"""..."""
        if c == 'r' && i + 1 < n && chars[i + 1] == '"' {
            let (text, ni, nl) = lex_raw(&chars, i + 1, line)?;
            out.push(Token { tok: Tok::PlainStr(text), line, col, spaced });
            line += nl;
            if nl > 0 {
                line_start = rewind_line_start(&chars, ni);
            }
            i = ni;
            spaced = false;
            continue;
        }
        if c == '"' {
            let (parts, ni, nl) = lex_dq(&chars, i, line)?;
            out.push(Token { tok: Tok::Str(parts), line, col, spaced });
            line += nl;
            if nl > 0 {
                line_start = rewind_line_start(&chars, ni);
            }
            i = ni;
            spaced = false;
            continue;
        }
        if c == '\'' {
            let (text, ni) = lex_sq(&chars, i, line)?;
            out.push(Token { tok: Tok::PlainStr(text), line, col, spaced });
            i = ni;
            spaced = false;
            continue;
        }
        if c == '`' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < n && chars[j] != '`' {
                name.push(chars[j]);
                j += 1;
            }
            if j >= n {
                return Err(LexError { line, msg: "unterminated backtick identifier".into() });
            }
            out.push(Token { tok: Tok::EscIdent(name), line, col, spaced });
            i = j + 1;
            spaced = false;
            continue;
        }
        if c.is_ascii_digit() {
            let (tok, ni) = lex_number(&chars, i, line)?;
            out.push(Token { tok, line, col, spaced });
            i = ni;
            spaced = false;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let name: String = chars[i..j].iter().collect();
            let tok = if c.is_uppercase() { Tok::TypeName(name) } else { Tok::Ident(name) };
            out.push(Token { tok, line, col, spaced });
            i = j;
            spaced = false;
            continue;
        }
        // symbols, longest first
        let mut matched = None;
        for s in SYMS {
            let sc: Vec<char> = s.chars().collect();
            if i + sc.len() <= n && chars[i..i + sc.len()] == sc[..] {
                matched = Some(*s);
                break;
            }
        }
        match matched {
            Some(s) => {
                out.push(Token { tok: Tok::Sym(s), line, col, spaced });
                i += s.chars().count();
                spaced = false;
            }
            None => {
                return Err(LexError { line, msg: format!("unexpected character {c:?}") });
            }
        }
    }
    out.push(Token { tok: Tok::Newline, line, col: 1, spaced: true });
    out.push(Token { tok: Tok::Eof, line, col: 1, spaced: true });
    Ok(out)
}

fn rewind_line_start(chars: &[char], i: usize) -> usize {
    let mut j = i;
    while j > 0 && chars[j - 1] != '\n' {
        j -= 1;
    }
    j
}

fn lex_number(chars: &[char], start: usize, line: usize) -> Result<(Tok, usize), LexError> {
    let n = chars.len();
    let mut i = start;
    // prefixed radix forms
    if chars[i] == '0' && i + 1 < n && matches!(chars[i + 1], 'x' | 'o' | 'b') {
        let radix = match chars[i + 1] {
            'x' => 16,
            'o' => 8,
            _ => 2,
        };
        i += 2;
        let mut v: i128 = 0;
        let mut any = false;
        while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
            if chars[i] == '_' {
                i += 1;
                continue;
            }
            let d = chars[i].to_digit(radix).ok_or_else(|| LexError { line, msg: format!("bad digit {:?} in radix literal", chars[i]) })?;
            v = v * radix as i128 + d as i128;
            any = true;
            if v > u64::MAX as i128 {
                return Err(LexError { line, msg: "integer literal out of range".into() });
            }
            i += 1;
        }
        if !any {
            return Err(LexError { line, msg: "radix literal without digits".into() });
        }
        // hex literals above i64::MAX are accepted by the lexer as wrapped u64
        // patterns only if representable — keep the ALS-E1 range rule: error.
        if v > i64::MAX as i128 {
            return Err(LexError { line, msg: "integer literal out of range".into() });
        }
        return Ok((Tok::Int(v as i64), i));
    }
    let mut text = String::new();
    while i < n && (chars[i].is_ascii_digit() || chars[i] == '_') {
        if chars[i] != '_' {
            text.push(chars[i]);
        }
        i += 1;
    }
    let mut is_float = false;
    // fraction: '.' followed by a digit (so `1..<5` and `t.0.1` lex as the grammar says)
    if i + 1 < n && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
        is_float = true;
        text.push('.');
        i += 1;
        while i < n && (chars[i].is_ascii_digit() || chars[i] == '_') {
            if chars[i] != '_' {
                text.push(chars[i]);
            }
            i += 1;
        }
    }
    // exponent
    if i < n && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < n && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < n && chars[j].is_ascii_digit() {
            is_float = true;
            text.push('e');
            if chars[i + 1] == '-' {
                text.push('-');
            }
            i = j;
            while i < n && chars[i].is_ascii_digit() {
                text.push(chars[i]);
                i += 1;
            }
        }
    }
    if is_float {
        return Ok((Tok::Float(text), i));
    }
    // decimal integer, ALS-E1: i64 range; the parser folds a preceding unary
    // minus so that i64::MIN is writable — the lexer therefore accepts up to
    // 9223372036854775808 and hands the magnitude over as Int(i64::MIN)
    // marker only when exactly that; anything larger is out of range.
    let mut v: i128 = 0;
    for ch in text.chars() {
        v = v * 10 + (ch as i128 - '0' as i128);
        if v > (i64::MAX as i128) + 1 {
            return Err(LexError { line, msg: "integer literal out of range".into() });
        }
    }
    if v == (i64::MAX as i128) + 1 {
        // only legal under a unary minus; the parser checks
        return Ok((Tok::Int(i64::MIN), i));
    }
    Ok((Tok::Int(v as i64), i))
}

fn lex_escape(chars: &[char], i: usize, line: usize) -> Result<(char, usize), LexError> {
    // chars[i] == '\\'
    let n = chars.len();
    if i + 1 >= n {
        return Err(LexError { line, msg: "dangling escape".into() });
    }
    let e = chars[i + 1];
    match e {
        'n' => Ok(('\n', i + 2)),
        't' => Ok(('\t', i + 2)),
        'r' => Ok(('\r', i + 2)),
        '0' => Ok(('\0', i + 2)),
        '\\' => Ok(('\\', i + 2)),
        '"' => Ok(('"', i + 2)),
        '\'' => Ok(('\'', i + 2)),
        '$' => Ok(('$', i + 2)),
        'x' => {
            if i + 3 >= n {
                return Err(LexError { line, msg: "bad \\x escape".into() });
            }
            let h: String = chars[i + 2..i + 4].iter().collect();
            let v = u32::from_str_radix(&h, 16).map_err(|_| LexError { line, msg: "bad \\x escape".into() })?;
            Ok((char::from_u32(v).unwrap_or('\u{FFFD}'), i + 4))
        }
        'u' => {
            if i + 2 < n && chars[i + 2] == '{' {
                let mut j = i + 3;
                let mut h = String::new();
                while j < n && chars[j] != '}' {
                    h.push(chars[j]);
                    j += 1;
                }
                let v = u32::from_str_radix(&h, 16).map_err(|_| LexError { line, msg: "bad \\u escape".into() })?;
                Ok((char::from_u32(v).unwrap_or('\u{FFFD}'), j + 1))
            } else {
                Err(LexError { line, msg: "bad \\u escape".into() })
            }
        }
        // ALS-E5: unknown escapes are OPEN (#1264); the accepted corpus is
        // what we follow — keep the backslash and the char.
        other => Ok((other, i + 2)),
    }
}

/// Double-quoted string (with `${}` interpolation) or heredoc `"""…"""`.
/// Returns (parts, next index, newlines consumed).
fn lex_dq(chars: &[char], start: usize, line: usize) -> Result<(Vec<StrPart>, usize, usize), LexError> {
    let n = chars.len();
    let heredoc = start + 2 < n && chars[start + 1] == '"' && chars[start + 2] == '"';
    let mut i = if heredoc { start + 3 } else { start + 1 };
    let mut parts: Vec<StrPart> = Vec::new();
    let mut text = String::new();
    let mut newlines = 0usize;
    loop {
        if i >= n {
            return Err(LexError { line, msg: "unterminated string".into() });
        }
        let c = chars[i];
        if heredoc {
            if c == '"' && i + 2 < n && chars[i + 1] == '"' && chars[i + 2] == '"' {
                i += 3;
                break;
            }
        } else if c == '"' {
            i += 1;
            break;
        } else if c == '\n' {
            return Err(LexError { line, msg: "newline in string literal".into() });
        }
        if c == '\n' {
            newlines += 1;
        }
        if c == '\\' {
            let (ch, ni) = lex_escape(chars, i, line)?;
            text.push(ch);
            i = ni;
            continue;
        }
        if c == '$' && i + 1 < n && chars[i + 1] == '{' {
            // collect balanced source up to the matching '}'
            let mut j = i + 2;
            let mut depth = 1usize;
            let mut src = String::new();
            while j < n {
                let d = chars[j];
                if d == '"' {
                    // nested string: copy verbatim through its end
                    let (_, nj, _) = lex_dq(chars, j, line)?;
                    src.extend(chars[j..nj].iter());
                    j = nj;
                    continue;
                }
                if d == '{' {
                    depth += 1;
                } else if d == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                src.push(d);
                j += 1;
            }
            if j >= n {
                return Err(LexError { line, msg: "unterminated ${ in string".into() });
            }
            if !text.is_empty() {
                parts.push(StrPart::Text(std::mem::take(&mut text)));
            }
            parts.push(StrPart::Interp(src));
            i = j + 1;
            continue;
        }
        text.push(c);
        i += 1;
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(StrPart::Text(text));
    }
    if heredoc {
        parts = dedent_heredoc(parts);
    }
    Ok((parts, i, newlines))
}

/// Heredoc normalization (GRAMMAR §Literals: "interpolation + common-indent
/// strip"; language.md §5.3 "Leading whitespace is stripped based on minimum
/// indent"): drop the first line if it is empty (text right after `"""`),
/// drop the trailing indentation-only line before the closing `"""`, then
/// remove the common leading-space count from every remaining line.
fn dedent_heredoc(parts: Vec<StrPart>) -> Vec<StrPart> {
    // Work on a flattened representation: interpolations become opaque
    // placeholders that never contain newlines.
    let mut flat = String::new();
    let mut interps: Vec<String> = Vec::new();
    for p in &parts {
        match p {
            StrPart::Text(t) => flat.push_str(t),
            StrPart::Interp(s) => {
                flat.push('\u{0}');
                interps.push(s.clone());
            }
        }
    }
    let mut lines: Vec<&str> = flat.split_inclusive('\n').collect();
    // first line empty (just the newline after the opening quotes)?
    if let Some(first) = lines.first() {
        if first.trim_end_matches(['\r', '\n']).is_empty() {
            lines.remove(0);
        }
    }
    // last line only indentation (before the closing quotes)?
    let mut strip_last = false;
    if let Some(last) = lines.last() {
        if !last.ends_with('\n') && last.chars().all(|c| c == ' ' || c == '\t') {
            strip_last = true;
        }
    }
    if strip_last {
        lines.pop();
        // and the newline that preceded it is not part of the value
        if let Some(last) = lines.last_mut() {
            let trimmed = last.trim_end_matches('\n');
            *last = trimmed;
        }
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim_end_matches(['\r', '\n']).is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        .min()
        .unwrap_or(0);
    let mut out = String::new();
    for l in lines {
        let body_len = l.trim_end_matches(['\r', '\n']).len();
        if body_len == 0 {
            out.push_str(l.trim_start_matches([' ', '\t']));
        } else {
            out.push_str(&l.chars().skip(indent).collect::<String>());
        }
    }
    // re-split on placeholders
    let mut res: Vec<StrPart> = Vec::new();
    let mut it = interps.into_iter();
    let mut buf = String::new();
    for c in out.chars() {
        if c == '\u{0}' {
            if !buf.is_empty() {
                res.push(StrPart::Text(std::mem::take(&mut buf)));
            }
            if let Some(s) = it.next() {
                res.push(StrPart::Interp(s));
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() || res.is_empty() {
        res.push(StrPart::Text(buf));
    }
    res
}

fn lex_sq(chars: &[char], start: usize, line: usize) -> Result<(String, usize), LexError> {
    let n = chars.len();
    let mut i = start + 1;
    let mut text = String::new();
    loop {
        if i >= n {
            return Err(LexError { line, msg: "unterminated string".into() });
        }
        let c = chars[i];
        if c == '\'' {
            return Ok((text, i + 1));
        }
        if c == '\\' {
            let (ch, ni) = lex_escape(chars, i, line)?;
            text.push(ch);
            i = ni;
            continue;
        }
        text.push(c);
        i += 1;
    }
}

/// Raw string after the `r`: `"…"` or `"""…"""`, no escapes, no interpolation.
fn lex_raw(chars: &[char], start: usize, line: usize) -> Result<(String, usize, usize), LexError> {
    let n = chars.len();
    let heredoc = start + 2 < n && chars[start + 1] == '"' && chars[start + 2] == '"';
    let mut i = if heredoc { start + 3 } else { start + 1 };
    let mut text = String::new();
    let mut newlines = 0usize;
    loop {
        if i >= n {
            return Err(LexError { line, msg: "unterminated raw string".into() });
        }
        let c = chars[i];
        if heredoc {
            if c == '"' && i + 2 < n && chars[i + 1] == '"' && chars[i + 2] == '"' {
                return Ok((text, i + 3, newlines));
            }
        } else if c == '"' {
            return Ok((text, i + 1, newlines));
        }
        if c == '\n' {
            newlines += 1;
        }
        text.push(c);
        i += 1;
    }
}
