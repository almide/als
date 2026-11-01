//! zlib (stdlib/zlib.almd): raw DEFLATE (RFC 1951), the zlib frame (RFC 1950)
//! and gzip members (RFC 1952). The compressors' bytes "vary by target"
//! (C-331's bounded divergence), so what every target shares is that the
//! decoders read any valid stream and every compressor's output decodes back
//! to its input. This evaluator's compressors emit stored blocks (RFC 1951
//! §3.2.4), a valid stream at every level. The inflater is a canonical-
//! Huffman reader written from the RFC, one bit at a time.
//!
//! Decode failures take C-331's canonical texts (stdlib/zlib_inflate.almd):
//! a stream that needs a byte past the input's end is `incomplete deflate
//! stream`; any other malformation, found where the decoder first meets it,
//! is `corrupt deflate stream` — an over-subscribed code, an incomplete one
//! (the literal/length and distance codes may be incomplete only when their
//! longest code is at most one bit; the code-length code never), too many
//! length or distance codes, a bad repeat, an invalid symbol, a distance
//! past the output. Output over the bound is `zlib: output exceeds N bytes`.
use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::value::Value;

pub const ZLIB_FNS: &[&str] = &[
    "zlib.deflate",
    "zlib.deflate_level",
    "zlib.inflate",
    "zlib.compress",
    "zlib.compress_level",
    "zlib.decompress",
    "zlib.gzip",
    "zlib.gunzip",
    "zlib.inflate_max",
    "zlib.decompress_max",
    "zlib.gunzip_max",
];

/// the documented output bound of the default entry points (R-zlib-04)
const LIMIT: usize = 268_435_456;

pub fn call_zlib(_it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !ZLIB_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(name, &args))
}

fn want_bytes(name: &str, args: &[Value], n: usize) -> Result<Vec<u8>, Flow> {
    if args.len() != n {
        return Err(Flow::Fatal(format!(
            "{name}: expected {n} argument(s), got {}",
            args.len()
        )));
    }
    match &args[0] {
        Value::Bytes(b) => Ok(b.borrow().clone()),
        other => Err(Flow::Abstain {
            class: "semantics:type-mismatch".into(),
            reason: format!("{name}: expected Bytes, got {}", other.type_name()),
        }),
    }
}

fn ok_bytes(v: Vec<u8>) -> Value {
    Value::Ok(Rc::new(Value::Bytes(Rc::new(std::cell::RefCell::new(v)))))
}

fn err_text(s: String) -> Value {
    Value::Err(Rc::new(Value::str(&s)))
}

fn exceeds(limit: usize) -> Value {
    err_text(format!("zlib: output exceeds {limit} bytes"))
}

/// the `_max` twins' bound; a non-positive one aborts (R-L0-33)
fn max_len(name: &str, args: &[Value]) -> Result<usize, Flow> {
    match args.get(1) {
        Some(Value::Int(n)) if *n > 0 => Ok(*n as usize),
        Some(Value::Int(n)) => Err(Flow::Panic(format!(
            "{name}: max_len must be positive, got {n}"
        ))),
        _ => Err(Flow::Fatal(format!("{name}: expected (Bytes, Int)"))),
    }
}

fn dispatch(name: &str, args: &[Value]) -> Result<Value, Flow> {
    match name {
        "zlib.deflate" => Ok(ok_bytes(stored(&want_bytes(name, args, 1)?))),
        "zlib.deflate_level" => Ok(ok_bytes(stored(&want_bytes(name, args, 2)?))),
        "zlib.compress" | "zlib.compress_level" => {
            let data = want_bytes(name, args, if name == "zlib.compress" { 1 } else { 2 })?;
            // CMF 0x78 (deflate, 32K window), FLG 0x01 (check bits, no dict)
            let mut out = vec![0x78, 0x01];
            out.extend(stored(&data));
            out.extend_from_slice(&adler32(&data).to_be_bytes());
            Ok(ok_bytes(out))
        }
        "zlib.gzip" => {
            let data = want_bytes(name, args, 1)?;
            let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
            out.extend(stored(&data));
            out.extend_from_slice(&crc32(&data).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            Ok(ok_bytes(out))
        }
        "zlib.inflate" | "zlib.inflate_max" => {
            let (data, limit) = if name == "zlib.inflate" {
                (want_bytes(name, args, 1)?, LIMIT)
            } else {
                (want_bytes(name, args, 2)?, max_len(name, args)?)
            };
            Ok(match inflate(&data, 0, limit) {
                Ok((_, after)) if after < data.len() => {
                    err_text("zlib.inflate: trailing data after stream".into())
                }
                Ok((out, _)) => ok_bytes(out),
                Err(Fail::Exceeds) => exceeds(limit),
                Err(f) => err_text(format!("zlib.inflate: {}", f.text())),
            })
        }
        "zlib.decompress" => Ok(decompress(&want_bytes(name, args, 1)?, LIMIT)),
        "zlib.decompress_max" => Ok(decompress(
            &want_bytes(name, args, 2)?,
            max_len(name, args)?,
        )),
        "zlib.gunzip" => Ok(gunzip(&want_bytes(name, args, 1)?, LIMIT)),
        "zlib.gunzip_max" => Ok(gunzip(&want_bytes(name, args, 2)?, max_len(name, args)?)),
        _ => Err(Flow::Fatal(format!("unrouted zlib fn `{name}`"))),
    }
}

fn decompress(d: &[u8], limit: usize) -> Value {
    let e = |t: &str| err_text(format!("zlib.decompress: {t}"));
    if d.len() < 2 {
        return e("unexpected end of file");
    }
    let (cmf, flg) = (u32::from(d[0]), u32::from(d[1]));
    if cmf & 15 != 8 || cmf >> 4 > 7 || (cmf * 256 + flg) % 31 != 0 {
        return e("invalid zlib header");
    }
    if flg & 32 != 0 {
        return e("preset dictionary unsupported");
    }
    match inflate(d, 2, limit) {
        Err(Fail::Exceeds) => exceeds(limit),
        Err(f) => e(f.text()),
        Ok((out, after)) => {
            if after + 4 > d.len() {
                e("unexpected end of file")
            } else if u32::from_be_bytes([d[after], d[after + 1], d[after + 2], d[after + 3]])
                != adler32(&out)
            {
                e("adler32 mismatch")
            } else if after + 4 < d.len() {
                e("trailing data after stream")
            } else {
                ok_bytes(out)
            }
        }
    }
}

/// gzip members back to back (a gzip file has at least one, so zero bytes
/// is `unexpected end of file`); whatever follows a member is another one
fn gunzip(d: &[u8], limit: usize) -> Value {
    let e = |t: &str| err_text(format!("zlib.gunzip: {t}"));
    if d.is_empty() {
        return e("unexpected end of file");
    }
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 0;
    while pos < d.len() {
        let start = match header_end(d, pos) {
            Ok(s) => s,
            Err(t) => return e(t),
        };
        // the bound is summed over the members
        let (member, after) = match inflate(d, start, limit - out.len()) {
            Ok(r) => r,
            Err(Fail::Exceeds) => return exceeds(limit),
            Err(f) => return e(f.text()),
        };
        if after + 8 > d.len() {
            return e("unexpected end of file");
        }
        let le = |p: usize| u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]);
        if crc32(&member) != le(after) {
            return e("crc mismatch");
        }
        if member.len() as u32 != le(after + 4) {
            return e("size mismatch");
        }
        out.extend(member);
        pos = after + 8;
    }
    ok_bytes(out)
}

/// where a member's deflate stream starts: magic 31 139 8 (a header cut off
/// while the magic still matches is truncation), FLG's reserved bits clear,
/// FEXTRA / FNAME / FCOMMENT skipped, FHCRC checked against the header's
/// CRC-32 low half
fn header_end(d: &[u8], pos: usize) -> Result<usize, &'static str> {
    let n = d.len();
    for (i, m) in [31u8, 139, 8].iter().enumerate() {
        if pos + i < n && d[pos + i] != *m {
            return Err("invalid gzip header");
        }
    }
    if pos + 10 > n {
        return Err("unexpected end of file");
    }
    let flg = d[pos + 3];
    let at = |p: usize| if p < n { d[p] } else { 0 };
    let mut p = pos + 10;
    if flg & 4 != 0 {
        p += 2 + (usize::from(at(p)) | usize::from(at(p + 1)) << 8);
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            while at(p) != 0 {
                p += 1;
            }
            p += 1;
        }
    }
    let end = if flg & 2 != 0 { p + 2 } else { p };
    if flg & 224 != 0 {
        return Err("invalid gzip header");
    }
    if end > n {
        return Err("unexpected end of file");
    }
    if flg & 2 != 0 && crc32(&d[pos..p]) & 0xffff != (u32::from(d[p]) | u32::from(d[p + 1]) << 8) {
        return Err("header crc mismatch");
    }
    Ok(end)
}

/// stored blocks of at most 65535 bytes; an empty input is one empty final block
fn stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5 * (data.len() / 65535 + 1));
    let mut chunks: Vec<&[u8]> = data.chunks(65535).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let last = chunks.len() - 1;
    for (i, c) in chunks.iter().enumerate() {
        out.push(u8::from(i == last));
        let n = c.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + u32::from(*x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for x in data {
        c ^= u32::from(*x);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

// ── the inflater (RFC 1951 §3.2) ─────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Fail {
    /// a byte past the input's end was needed
    Incomplete,
    Corrupt,
    Exceeds,
}

impl Fail {
    fn text(self) -> &'static str {
        match self {
            Fail::Incomplete => "incomplete deflate stream",
            Fail::Corrupt => "corrupt deflate stream",
            Fail::Exceeds => "output exceeds the bound",
        }
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    cnt: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<u32, Fail> {
        let mut v = self.bit;
        while self.cnt < n {
            if self.pos >= self.d.len() {
                return Err(Fail::Incomplete);
            }
            v |= u32::from(self.d[self.pos]) << self.cnt;
            self.pos += 1;
            self.cnt += 8;
        }
        self.bit = v >> n;
        self.cnt -= n;
        Ok(v & ((1u32 << n) - 1))
    }
}

/// canonical Huffman table: count per length, symbols by code order
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

/// zlib's acceptance rule (inftrees.c / miniz_oxide): never over-subscribed;
/// incomplete only when `sparse` and the longest code is at most one bit
fn code_ok(lengths: &[u8], sparse: bool) -> bool {
    let mut count = [0i64; 16];
    for l in lengths {
        count[usize::from(*l)] += 1;
    }
    let (mut left, mut max_len) = (1i64, 0usize);
    for (len, c) in count.iter().enumerate().skip(1) {
        left = left * 2 - c;
        if *c > 0 {
            max_len = len;
        }
        if left < 0 {
            return false;
        }
    }
    left == 0 || (sparse && max_len <= 1)
}

fn build(lengths: &[u8]) -> Huff {
    let mut count = [0u16; 16];
    for l in lengths {
        count[usize::from(*l)] += 1;
    }
    let mut offs = [0u16; 16];
    for len in 1..15 {
        offs[len + 1] = offs[len] + count[len];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (s, l) in lengths.iter().enumerate() {
        if *l != 0 {
            symbol[usize::from(offs[usize::from(*l)])] = s as u16;
            offs[usize::from(*l)] += 1;
        }
    }
    count[0] = 0;
    Huff { count, symbol }
}

/// the symbol, or None when no code of 15 bits or fewer matches
fn decode(b: &mut Bits, h: &Huff) -> Result<Option<u16>, Fail> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= b.need(1)? as i32;
        let count = i32::from(h.count[len]);
        if code - count < first {
            return Ok(Some(h.symbol[(index + (code - first)) as usize]));
        }
        index += count;
        first = (first + count) << 1;
        code <<= 1;
    }
    Ok(None)
}

const LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEXT: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn codes(
    b: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huff,
    dist: &Huff,
    limit: usize,
) -> Result<(), Fail> {
    loop {
        let sym = match decode(b, lit)? {
            Some(s) => usize::from(s),
            None => return Err(Fail::Corrupt),
        };
        if sym < 256 {
            if out.len() >= limit {
                return Err(Fail::Exceeds);
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else if sym - 257 >= 29 {
            return Err(Fail::Corrupt);
        } else {
            let li = sym - 257;
            let len = usize::from(LBASE[li]) + b.need(u32::from(LEXT[li]))? as usize;
            let ds = decode(b, dist)?.map(usize::from);
            let dsc = match ds {
                Some(x) if x < 30 => x,
                _ => 0,
            };
            let d = usize::from(DBASE[dsc]) + b.need(u32::from(DEXT[dsc]))? as usize;
            if ds.is_none_or(|x| x >= 30) || d > out.len() {
                return Err(Fail::Corrupt);
            }
            if out.len() + len > limit {
                return Err(Fail::Exceeds);
            }
            for _ in 0..len {
                let x = out[out.len() - d];
                out.push(x);
            }
        }
    }
}

fn dynamic(b: &mut Bits, out: &mut Vec<u8>, limit: usize) -> Result<(), Fail> {
    let nlen = b.need(5)? as usize + 257;
    let ndist = b.need(5)? as usize + 1;
    let ncode = b.need(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(Fail::Corrupt);
    }
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl = [0u8; 19];
    for o in ORDER.iter().take(ncode) {
        cl[*o] = b.need(3)? as u8;
    }
    if !code_ok(&cl, false) {
        return Err(Fail::Corrupt);
    }
    let clh = build(&cl);
    let total = nlen + ndist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = decode(b, &clh)?;
        // the repeat's extra bits are read before the repeat is judged
        let (val, rep) = match sym {
            Some(s) if s < 16 => (s as u8, 1),
            Some(16) => {
                let extra = b.need(2)? as usize;
                if i == 0 {
                    return Err(Fail::Corrupt);
                }
                (lengths[i - 1], 3 + extra)
            }
            Some(17) => (0, 3 + b.need(3)? as usize),
            Some(18) => (0, 11 + b.need(7)? as usize),
            _ => return Err(Fail::Corrupt),
        };
        if i + rep > total {
            return Err(Fail::Corrupt);
        }
        for _ in 0..rep {
            lengths[i] = val;
            i += 1;
        }
    }
    if !code_ok(&lengths[..nlen], true) || !code_ok(&lengths[nlen..], true) {
        return Err(Fail::Corrupt);
    }
    codes(
        b,
        out,
        &build(&lengths[..nlen]),
        &build(&lengths[nlen..]),
        limit,
    )
}

/// one DEFLATE stream from byte `start`: (decoded bytes, first byte after it)
fn inflate(d: &[u8], start: usize, limit: usize) -> Result<(Vec<u8>, usize), Fail> {
    let mut b = Bits {
        d,
        pos: start,
        bit: 0,
        cnt: 0,
    };
    let mut out = Vec::new();
    let mut fixed_lit = [8u8; 288];
    for (i, x) in fixed_lit.iter_mut().enumerate() {
        *x = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    loop {
        let last = b.need(1)?;
        match b.need(2)? {
            0 => {
                b.bit = 0;
                b.cnt = 0;
                let at = |p: usize| if p < d.len() { usize::from(d[p]) } else { 0 };
                let n = at(b.pos) | at(b.pos + 1) << 8;
                let nn = at(b.pos + 2) | at(b.pos + 3) << 8;
                let begin = b.pos + 4;
                let held = n.min(d.len().saturating_sub(begin));
                if n ^ nn != 0xffff {
                    // a cut-off header reads as zeros: truncation first
                    return Err(if begin > d.len() {
                        Fail::Incomplete
                    } else {
                        Fail::Corrupt
                    });
                }
                if out.len() + held > limit {
                    return Err(Fail::Exceeds);
                }
                if begin + n > d.len() {
                    return Err(Fail::Incomplete);
                }
                out.extend_from_slice(&d[begin..begin + n]);
                b.pos = begin + n;
            }
            // all 32 fixed distance codes: 30 and 31 are valid codes for
            // invalid symbols (corrupt), not codes the decoder cannot finish
            1 => codes(
                &mut b,
                &mut out,
                &build(&fixed_lit),
                &build(&[5u8; 32]),
                limit,
            )?,
            2 => dynamic(&mut b, &mut out, limit)?,
            _ => return Err(Fail::Corrupt),
        }
        if last == 1 {
            return Ok((out, b.pos));
        }
    }
}
