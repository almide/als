//! Round-2 batch B: transcendentals over the vendored musl libm, the civil
//! calendar (translated from stdlib/datetime_calendar.almd — the normative
//! self-hosted source), hashes, base64, hex, path, args, random, io/fs
//! leftovers, the bytes cursor writers, and the offline http builders.
//! Same doctrine: chapters + fixtures decide; unknown edges abstain.

use std::rc::Rc;

use crate::eval::{fnan, Flow, Interp};
use crate::libm;
use crate::stdlib_ext::{obs_bits_f32, obs_bits_f64};
use crate::value::{Value, F64};

pub const EXT2_FNS: &[&str] = &[
    "math.exp",
    "math.log",
    "math.log2",
    "math.log10",
    "math.sin",
    "math.cos",
    "math.tan",
    "math.atan",
    "math.tanh",
    "math.expm1",
    "math.e",
    "math.pi",
    "math.log_gamma",
    "math.choose",
    "math.factorial",
    "datetime.year",
    "datetime.month",
    "datetime.day",
    "datetime.hour",
    "datetime.minute",
    "datetime.second",
    "datetime.weekday",
    "datetime.from_parts",
    "datetime.to_iso",
    "datetime.format",
    "datetime.parse_iso",
    "url.parse",
    "url.to_string",
    "url.resolve",
    "url.parse_query",
    "hash.fnv1a32",
    "hash.fnv1a32_bytes",
    "hash.sha256",
    "hash.sha256_hex",
    "hash.sha512",
    "hash.sha512_hex",
    "hash.hmac_sha256",
    "hash.hmac_sha256_hex",
    "hash.hmac_sha512",
    "hash.hmac_sha512_hex",
    "hash.is_valid_hmac_sha256",
    "hash.is_valid_hmac_sha512",
    "bytes.is_equal_constant_time",
    "string.is_equal_constant_time",
    "secret.new",
    "secret.reveal",
    "base64.encode",
    "base64.encode_url",
    "base64.decode",
    "base64.decode_url",
    "hex.encode",
    "hex.encode_upper",
    "hex.decode",
    "int.to_hex",
    "path.join",
    "path.basename",
    "path.dirname",
    "path.extension",
    "path.is_absolute",
    "path.from_string",
    "path.trusted",
    "path.to_string",
    "path.join_within",
    "args.raw",
    "args.flag",
    "args.option",
    "args.option_or",
    "args.positional",
    "args.positional_at",
    "random.int",
    "random.bytes",
    "io.write",
    "io.write_bytes",
    "io.read_n_bytes",
    "io.read_all",
    "io.read_line",
    "fs.read_bytes_raw",
    "fs.read_bytes_raw_if_exists",
    "fs.write_bytes_raw",
    "fs.read_lines_if_exists",
    "fs.read_bytes_if_exists",
    "fs.modified_at",
    "fs.walk",
    "bytes.pad_left",
    "bytes.pad_right",
    "bytes.chunks",
    "bytes.copy_within",
    "bytes.write_u8",
    "bytes.write_u16_le",
    "bytes.write_u16_be",
    "bytes.write_u32_le",
    "bytes.write_u32_be",
    "bytes.write_i32_le",
    "bytes.write_i32_be",
    "bytes.write_i64_le",
    "bytes.write_i64_be",
    "bytes.write_f32_le",
    "bytes.write_f64_be",
    "bytes.write_bool",
    "bytes.write_string_be",
    "http.response",
    "http.json",
    "http.new_request",
    "http.query_params",
    "http.redirect",
    "http.with_headers",
    "http.get_header",
    "http.set_header",
    "http.status",
    "http.body",
    "string.run_length_encode",
    "list.length",
    "list.tail",
    "list.with_capacity",
    "list.window",
    "list.binary_search",
    "result.flatten",
    "result.or_else",
    "map.upsert",
    "fan.map",
    "fan.settle",
];

// small local mirrors of the arg helpers (stdlib_ext keeps its own)
fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() != n {
        // The implementation type-checked this call, so a count the
        // evaluator does not model is an OVERLOAD it has not implemented
        // (bytes.set_int32's 4-argument endian form, C-213) — an abstain
        // under the stdlib class, never a fault (totality-or-abstain).
        return Err(Flow::Abstain {
            class: format!("stdlib:{name}"),
            reason: format!(
                "{name}: expected {n} argument(s), got {} — unmodelled overload",
                args.len()
            ),
        });
    }
    Ok(())
}

fn mismatch<T>(name: &str, want: &str, got: &Value) -> Result<T, Flow> {
    Err(Flow::Abstain {
        class: "semantics:type-mismatch".into(),
        reason: format!("{name}: expected {want}, got {}", got.type_name()),
    })
}

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<str>, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => mismatch(name, "String", other),
    }
}

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => mismatch(name, "Int", other),
    }
}

fn want_float(name: &str, v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(f) => Ok(f.0),
        other => mismatch(name, "Float", other),
    }
}

fn want_list<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<Value>>, Flow> {
    match v {
        Value::List(xs) => Ok(xs),
        other => mismatch(name, "List", other),
    }
}

fn want_bytes(name: &str, v: &Value) -> Result<Rc<std::cell::RefCell<Vec<u8>>>, Flow> {
    match v {
        Value::Bytes(b) => Ok(b.clone()),
        other => mismatch(name, "Bytes", other),
    }
}

fn bytes_val(v: Vec<u8>) -> Value {
    Value::Bytes(Rc::new(std::cell::RefCell::new(v)))
}

/// C-432: the path stays inside the directory it is joined to.
fn path_stays_inside(s: &str) -> bool {
    let cs: Vec<char> = s.chars().collect();
    if matches!(cs.first(), Some('/' | '\\' | '~')) || cs.contains(&'\0') {
        return false;
    }
    // The first segment (up to either separator) is a drive when its second
    // char is `:`.
    let first_len = cs
        .iter()
        .position(|c| *c == '/' || *c == '\\')
        .unwrap_or(cs.len());
    if first_len >= 2 && cs[1] == ':' {
        return false;
    }
    let mut seg: Vec<char> = Vec::new();
    for c in cs.iter().chain(std::iter::once(&'/')) {
        if *c == '/' || *c == '\\' {
            if seg.len() == 2 && seg[0] == '.' && seg[1] == '.' {
                return false;
            }
            seg.clear();
        } else {
            seg.push(*c);
        }
    }
    true
}

fn ok(v: Value) -> Value {
    Value::Ok(Rc::new(v))
}

fn err_str(s: &str) -> Value {
    Value::Err(Rc::new(Value::str(s)))
}

fn some(v: Value) -> Value {
    Value::Some(Rc::new(v))
}

fn fl(x: f64) -> Result<Value, Flow> {
    Ok(Value::Float(F64(fnan(x))))
}

fn io_err(e: std::io::Error) -> Value {
    err_str(&e.to_string())
}

pub fn call_ext2(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    match dispatch(it, name, args) {
        Ok(r) => Some(r),
        Err(Flow::Fatal(m)) if &*m == "__not_ext__" => None,
        Err(f) => Some(Err(f)),
    }
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Result<Value, Flow>, Flow> {
    Ok(match name {
        // ═══ math over vendored musl libm (bit-pinned: math_transcendental_bits) ═══
        "math.exp" => fl(libm::almide_rt_libm_exp(want_float(name, &args[0])?)),
        "math.log" => fl(libm::almide_rt_libm_log(want_float(name, &args[0])?)),
        "math.log2" => fl(libm::almide_rt_libm_log2(want_float(name, &args[0])?)),
        "math.log10" => fl(libm::almide_rt_libm_log10(want_float(name, &args[0])?)),
        "math.sin" => fl(libm::almide_rt_libm_sin(want_float(name, &args[0])?)),
        "math.cos" => fl(libm::almide_rt_libm_cos(want_float(name, &args[0])?)),
        "math.tan" => fl(libm::almide_rt_libm_tan(want_float(name, &args[0])?)),
        "math.atan" => fl(libm::almide_rt_libm_atan(want_float(name, &args[0])?)),
        "math.tanh" => fl(libm::almide_rt_libm_tanh(want_float(name, &args[0])?)),
        "math.expm1" => fl(libm::almide_rt_libm_expm1(want_float(name, &args[0])?)),
        "math.fpow" => fl(libm::almide_rt_libm_pow(
            want_float(name, &args[0])?,
            want_float(name, &args[1])?,
        )),
        "math.e" => {
            arity(name, &args, 0)?;
            fl(std::f64::consts::E)
        }
        "math.pi" => {
            arity(name, &args, 0)?;
            fl(std::f64::consts::PI)
        }
        "math.log_gamma" => {
            // translated from stdlib/math_lgamma.almd: Lanczos g=7 n=9 with
            // exact bit constants, ln routed through the vendored libm log.
            // Below 0.5 the series is invalid (its log arguments cross zero),
            // so the REFLECTION formula ln|G(x)| = ln(pi / |sin(pi x)|) - ln G(1 - x)
            // carries the work to 1 - x, with |sin(pi x)| on the reduced
            // fraction; the non-positive integers and +-inf are poles (C-358).
            arity(name, &args, 1)?;
            let x0 = want_float(name, &args[0])?;
            if x0 < 0.5 {
                let y = if x0 < 0.0 { -x0 } else { x0 };
                let f0 = y - y.floor();
                let f = if f0 > 0.5 { 1.0 - f0 } else { f0 };
                let sn = libm::almide_rt_libm_sin(std::f64::consts::PI * f);
                // a pole (sn == 0) or -inf (its fraction is NaN, so sn is NaN)
                if sn == 0.0 || sn.is_nan() {
                    return Ok(Ok(Value::Float(F64(f64::INFINITY))));
                }
                let refl = libm::almide_rt_libm_log(std::f64::consts::PI / sn);
                return Ok(Ok(Value::Float(F64(refl - lanczos_log_gamma(1.0 - x0)))));
            }
            if x0 == f64::INFINITY {
                return Ok(Ok(Value::Float(F64(f64::INFINITY))));
            }
            return Ok(Ok(Value::Float(F64(lanczos_log_gamma(x0)))));
        }
        "math.choose" => {
            // C(n, k) EXACTLY whenever it fits in an Int, its wrap mod 2^64
            // otherwise (C-358): each factor's power of two is counted and its
            // odd part multiplied in, and the odd denominator is divided out at
            // the end by its Newton inverse mod 2^64 — a wrapped product cannot
            // be divided, which is what made the old multiply-before-divide
            // running product answer -284401161134521734 for choose(62, 31).
            arity(name, &args, 2)?;
            let (n, k) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            if k < 0 || k > n {
                return Ok(Ok(Value::Int(0)));
            }
            let k = k.min(n.wrapping_sub(k));
            let (mut odd_num, mut odd_den): (u64, u64) = (1, 1);
            let mut twos: u32 = 0;
            let mut i: i64 = 0;
            while i < k {
                let a = (n - i) as u64;
                let b = (i + 1) as u64;
                let (ta, tb) = (a.trailing_zeros(), b.trailing_zeros());
                odd_num = odd_num.wrapping_mul(a >> ta);
                odd_den = odd_den.wrapping_mul(b >> tb);
                twos = twos + ta - tb;
                i += 1;
            }
            let mut inv = odd_den;
            for _ in 0..5 {
                inv = inv.wrapping_mul(2u64.wrapping_sub(odd_den.wrapping_mul(inv)));
            }
            let odd = odd_num.wrapping_mul(inv);
            Ok(Value::Int(
                (if twos >= 64 { 0 } else { odd << twos }) as i64,
            ))
        }
        "math.factorial" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            let mut acc: i64 = 1;
            let mut i: i64 = 2;
            while i <= n {
                acc = acc.wrapping_mul(i);
                i += 1;
            }
            Ok(Value::Int(acc))
        }

        // ═══ datetime — the civil calendar, translated verbatim from
        //     stdlib/datetime_calendar.almd (wrapping i64 + truncating /) ═══
        "datetime.year" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(civ(want_int(name, &args[0])?).0))
        }
        "datetime.month" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(civ(want_int(name, &args[0])?).1))
        }
        "datetime.day" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(civ(want_int(name, &args[0])?).2))
        }
        "datetime.hour" | "datetime.minute" | "datetime.second" => {
            arity(name, &args, 1)?;
            let ts = want_int(name, &args[0])?;
            let sod = ts
                .wrapping_rem(86400)
                .wrapping_add(86400)
                .wrapping_rem(86400);
            Ok(Value::Int(match name {
                "datetime.hour" => sod / 3600,
                "datetime.minute" => sod % 3600 / 60,
                _ => sod % 60,
            }))
        }
        "datetime.weekday" => {
            arity(name, &args, 1)?;
            let ts = want_int(name, &args[0])?;
            let idx = days_from_epoch(ts).rem_euclid(7);
            const WD: [&str; 7] = [
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
            ];
            Ok(Value::str(WD[idx as usize]))
        }
        // ALS-T26 (C-360, #4149): the same range rules and texts as parse_iso,
        // read from the one check both constructors share; nothing rolls over.
        "datetime.from_parts" => {
            arity(name, &args, 6)?;
            let mut a = [0i64; 6];
            for (i, slot) in a.iter_mut().enumerate() {
                *slot = want_int(name, &args[i])?;
            }
            Ok(match iso_civil(a[0], a[1], a[2], a[3], a[4], a[5]) {
                Ok(ts) => Value::Ok(Rc::new(Value::Int(ts))),
                Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
            })
        }
        "datetime.to_iso" => {
            arity(name, &args, 1)?;
            let ts = want_int(name, &args[0])?;
            let (y, mo, d) = civ(ts);
            let sod = ts
                .wrapping_rem(86400)
                .wrapping_add(86400)
                .wrapping_rem(86400);
            let (h, mi, s) = (sod / 3600, sod % 3600 / 60, sod % 60);
            let ystr = iso_year(y);
            Ok(Value::str(&format!(
                "{ystr}-{}-{}T{}:{}:{}Z",
                pad_num(mo, 2),
                pad_num(d, 2),
                pad_num(h, 2),
                pad_num(mi, 2),
                pad_num(s, 2)
            )))
        }
        // ALS-T26: date "T" time zone?, every field ASCII digits of any width,
        // the offset APPLIED, each range refusal naming its field. Written from
        // the chapter, not from the implementation (ADR-0015).
        "datetime.parse_iso" => {
            arity(name, &args, 1)?;
            let raw = want_str(name, &args[0])?.to_string();
            Ok(match iso_parse(&raw) {
                Ok(ts) => Value::Ok(Rc::new(Value::Int(ts))),
                Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
            })
        }
        // ALS-T27: the host is checked AFTER the host:port split; userinfo and
        // IPv6 literals are refused by name; the port is ASCII digits 0..=65535.
        "url.parse" => {
            arity(name, &args, 1)?;
            let raw = want_str(name, &args[0])?.to_string();
            Ok(match url_parse(&raw) {
                Ok(v) => Value::Ok(Rc::new(v)),
                Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
            })
        }
        "url.resolve" => {
            arity(name, &args, 2)?;
            let raw = want_str(name, &args[1])?.to_string();
            Ok(match url_resolve(&args[0], &raw) {
                Ok(v) => Value::Ok(Rc::new(v)),
                Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
            })
        }
        "url.parse_query" => {
            arity(name, &args, 1)?;
            let raw = want_str(name, &args[0])?.to_string();
            Ok(match url_parse_query(&raw, "url.parse_query") {
                Ok(ps) => Value::Ok(Rc::new(pairs_value(ps))),
                Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
            })
        }
        "url.to_string" => {
            arity(name, &args, 1)?;
            let (scheme, host, port, path, query, fragment) = url_parts(name, &args[0])?;
            let mut out = format!("{scheme}://{host}");
            if let Some(p) = port {
                out.push_str(&format!(":{p}"));
            }
            out.push_str(&path);
            if !query.is_empty() {
                out.push('?');
                out.push_str(&query);
            }
            if !fragment.is_empty() {
                out.push('#');
                out.push_str(&fragment);
            }
            Ok(Value::str(&out))
        }
        "datetime.format" => {
            // stdlib/datetime_format.almd: six sequential string.replace calls;
            // %% and unknown tokens pass through untouched
            arity(name, &args, 2)?;
            let ts = want_int(name, &args[0])?;
            let pattern = want_str(name, &args[1])?.to_string();
            let (y, mo, d) = civ(ts);
            let sod = ts
                .wrapping_rem(86400)
                .wrapping_add(86400)
                .wrapping_rem(86400);
            // %Y is the same variable-width year to_iso renders (ALS-T17,
            // C-359): four columns minimum, zero-padded, the sign taking one.
            let out = pattern
                .replace("%Y", &iso_year(y))
                .replace("%m", &pad_num(mo, 2))
                .replace("%d", &pad_num(d, 2))
                .replace("%H", &pad_num(sod / 3600, 2))
                .replace("%M", &pad_num(sod % 3600 / 60, 2))
                .replace("%S", &pad_num(sod % 60, 2));
            Ok(Value::str(&out))
        }

        // ═══ hashes ══════════════════════════════════════════════════════
        "hash.fnv1a32" | "hash.fnv1a32_bytes" => {
            arity(name, &args, 1)?;
            let bytes: Vec<u8> = if name == "hash.fnv1a32" {
                want_str(name, &args[0])?.as_bytes().to_vec()
            } else {
                want_bytes(name, &args[0])?.borrow().clone()
            };
            let mut h: u32 = 2166136261;
            for b in bytes {
                h ^= b as u32;
                h = h.wrapping_mul(16777619);
            }
            Ok(Value::Int(h as i64))
        }
        "hash.sha256" | "hash.sha256_hex" => {
            arity(name, &args, 1)?;
            let bytes: Vec<u8> = if name == "hash.sha256_hex" {
                want_str(name, &args[0])?.as_bytes().to_vec()
            } else {
                want_bytes(name, &args[0])?.borrow().clone()
            };
            let digest = sha256(&bytes);
            Ok(if name == "hash.sha256_hex" {
                Value::str(&hex_lower(&digest))
            } else {
                bytes_val(digest.to_vec())
            })
        }
        "hash.sha512" | "hash.sha512_hex" => {
            arity(name, &args, 1)?;
            let digest = sha512(&want_bytes(name, &args[0])?.borrow());
            Ok(if name == "hash.sha512_hex" {
                Value::str(&hex_lower(&digest))
            } else {
                bytes_val(digest.to_vec())
            })
        }
        // HMAC (RFC 2104 / RFC 4231): the key comes first, as RFC 2104 writes
        // HMAC(K, m); a key longer than the block is hashed first.
        "hash.hmac_sha256"
        | "hash.hmac_sha256_hex"
        | "hash.hmac_sha512"
        | "hash.hmac_sha512_hex" => {
            arity(name, &args, 2)?;
            let key = want_bytes(name, &args[0])?.borrow().clone();
            let msg = want_bytes(name, &args[1])?.borrow().clone();
            let mac = hmac(name.contains("512"), &key, &msg);
            Ok(if name.ends_with("_hex") {
                Value::str(&hex_lower(&mac))
            } else {
                bytes_val(mac)
            })
        }
        // The verify cells answer whether `tag` is the MAC of b under key; a
        // tag whose length differs from the MAC's answers false. Timing is
        // not an observable the reference can judge; the answer is.
        "hash.is_valid_hmac_sha256" | "hash.is_valid_hmac_sha512" => {
            arity(name, &args, 3)?;
            let key = want_bytes(name, &args[0])?.borrow().clone();
            let msg = want_bytes(name, &args[1])?.borrow().clone();
            let mac = hmac(name.contains("512"), &key, &msg);
            let tag = want_bytes(name, &args[2])?.borrow().clone();
            Ok(Value::Bool(tag == mac))
        }
        // Constant-time equality: the answer is plain equality of the bytes
        // (a String compares its UTF-8); lengths are public.
        "bytes.is_equal_constant_time" => {
            arity(name, &args, 2)?;
            let a = want_bytes(name, &args[0])?.borrow().clone();
            let b = want_bytes(name, &args[1])?.borrow().clone();
            Ok(Value::Bool(a == b))
        }
        "string.is_equal_constant_time" => {
            arity(name, &args, 2)?;
            let a = want_str(name, &args[0])?.clone();
            let b = want_str(name, &args[1])?.clone();
            Ok(Value::Bool(a.as_bytes() == b.as_bytes()))
        }

        // ═══ secret (C-425..C-427): an opaque holder; reveal is the one way out
        "secret.new" => {
            arity(name, &args, 1)?;
            Ok(crate::value::secret_of(args.into_iter().next().unwrap()))
        }
        "secret.reveal" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Record {
                    type_name: Some(t),
                    fields,
                } if &**t == crate::value::SECRET_TYPE => Ok(fields[0].1.clone()),
                other => Err(Flow::Fatal(format!(
                    "secret.reveal: expected Secret, got {}",
                    other.type_name()
                ))),
            }
        }

        // ═══ base64 (RFC 4648, canonical padded; one alphabet per decoder) ═══
        "base64.encode" | "base64.encode_url" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let s = b64_encode(&b.borrow(), name.ends_with("url"));
            Ok(Value::str(&s))
        }
        "base64.decode" | "base64.decode_url" => {
            arity(name, &args, 1)?;
            let s = want_str(name, &args[0])?;
            Ok(match b64_decode(s, name.ends_with("url")) {
                Ok(v) => ok(bytes_val(v)),
                Err(m) => err_str(&m),
            })
        }

        // ═══ hex ═════════════════════════════════════════════════════════
        "hex.encode" | "hex.encode_upper" => {
            arity(name, &args, 1)?;
            let h = {
                let b = want_bytes(name, &args[0])?;
                let h = hex_lower(&b.borrow());
                h
            };
            Ok(Value::str(&if name.ends_with("upper") {
                // manual a-f → A-F (the case-mapping lint wall stays intact)
                h.chars()
                    .map(|c| {
                        if c.is_ascii_lowercase() {
                            (c as u8 - 32) as char
                        } else {
                            c
                        }
                    })
                    .collect::<String>()
            } else {
                h
            }))
        }
        "hex.decode" => {
            arity(name, &args, 1)?;
            let s = want_str(name, &args[0])?;
            let cs: Vec<char> = s.chars().collect();
            if !cs.len().is_multiple_of(2) {
                return Ok(Ok(err_str("invalid hex")));
            }
            let mut out = Vec::with_capacity(cs.len() / 2);
            for pair in cs.chunks(2) {
                match (pair[0].to_digit(16), pair[1].to_digit(16)) {
                    (Some(a), Some(b)) => out.push((a * 16 + b) as u8),
                    _ => return Ok(Ok(err_str("invalid hex"))),
                }
            }
            Ok(ok(bytes_val(out)))
        }
        "int.to_hex" => {
            // stdlib/int_hex.almd: 0 → "0", negative → all 16 nibbles, lowercase
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            let count = if n == 0 {
                1
            } else if n < 0 {
                16
            } else {
                let mut c = 0;
                let mut v = n;
                while v != 0 {
                    v /= 16;
                    c += 1;
                }
                c
            };
            let mut out = String::new();
            for i in 0..count {
                let pos = count - 1 - i;
                let nib = ((n as u64) >> (pos * 4)) & 15;
                out.push(char::from_digit(nib as u32, 16).unwrap_or('?'));
            }
            Ok(Value::str(&out))
        }

        // ═══ path (pure string ops) ══════════════════════════════════════
        "path.join" => {
            arity(name, &args, 2)?;
            let a = want_str(name, &args[0])?.to_string();
            let b = want_str(name, &args[1])?.to_string();
            Ok(Value::str(&if b.starts_with('/') || a.is_empty() {
                b
            } else if a.ends_with('/') {
                format!("{a}{b}")
            } else {
                format!("{a}/{b}")
            }))
        }
        "path.basename" | "path.dirname" => {
            arity(name, &args, 1)?;
            let p = want_str(name, &args[0])?;
            Ok(Value::str(match p.rfind('/') {
                Some(k) if name == "path.basename" => &p[k + 1..],
                Some(0) if name == "path.dirname" => "/",
                Some(k) => &p[..k],
                None if name == "path.basename" => p,
                None => ".",
            }))
        }
        "path.extension" => {
            arity(name, &args, 1)?;
            let p = want_str(name, &args[0])?;
            let base = match p.rfind('/') {
                Some(k) => &p[k + 1..],
                None => &p[..],
            };
            Ok(match base.rfind('.') {
                Some(k) if k > 0 => some(Value::str(&base[k + 1..])),
                _ => Value::None,
            })
        }
        "path.is_absolute" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_str(name, &args[0])?.starts_with('/')))
        }
        // SafePath (C-432): admitted only when the path stays inside the
        // directory it is joined to — relative (no leading `/` or `\`, no
        // `X:` drive, no leading `~`), no NUL, no `..` segment on either
        // separator. A SafePath is its string.
        "path.from_string" | "path.join_within" => {
            let (base, s) = if name == "path.from_string" {
                arity(name, &args, 1)?;
                (None, want_str(name, &args[0])?.to_string())
            } else {
                arity(name, &args, 2)?;
                (
                    Some(want_str(name, &args[0])?.to_string()),
                    want_str(name, &args[1])?.to_string(),
                )
            };
            Ok(if !path_stays_inside(&s) {
                err_str(&format!("path traversal rejected: {s}"))
            } else {
                ok(match base {
                    None => Value::str(&s),
                    Some(a) => Value::str(&if a.is_empty() {
                        s
                    } else if a.ends_with('/') {
                        format!("{a}{s}")
                    } else {
                        format!("{a}/{s}")
                    }),
                })
            })
        }
        "path.trusted" | "path.to_string" => {
            arity(name, &args, 1)?;
            Ok(Value::str(want_str(name, &args[0])?))
        }

        // ═══ args (stdlib/args.almd over env.args(); ref argv is empty) ═══
        "args.raw" => {
            arity(name, &args, 0)?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        // positional(options) names the options whose values it leaves out (#4144)
        "args.positional" => {
            arity(name, &args, 1)?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "args.flag" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(false))
        }
        "args.option" => {
            arity(name, &args, 1)?;
            Ok(Value::None)
        }
        "args.positional_at" => {
            arity(name, &args, 2)?;
            Ok(Value::None)
        }
        "args.option_or" => {
            arity(name, &args, 2)?;
            Ok(args[1].clone())
        }

        // ═══ random (entropy floor; ref uses a fixed-seed splitmix64 —
        //     the VALUE is sanctioned nondeterminism, ALS asserts the range) ═══
        "random.int" => {
            arity(name, &args, 2)?;
            let (min, max) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            let span = max.wrapping_sub(min).wrapping_add(1);
            let r = it.next_rand() as i64;
            let rpos = r & 0x7FFF_FFFF_FFFF_FFFF;
            Ok(Value::Int(if span <= 0 {
                min
            } else {
                min.wrapping_add(rpos % span)
            }))
        }

        // C-394: the CSPRNG surface. The bytes are sanctioned nondeterminism
        // (the ref draws them from its splitmix64); ALS asserts the length,
        // the empty case, two draws differing, and the negative-length abort.
        "random.bytes" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            if n < 0 {
                return Ok(Err(Flow::Abort(format!(
                    "random.bytes: negative length {n}"
                ))));
            }
            if n > (1i64 << 31) {
                return Ok(Err(Flow::Abort("out of memory".into())));
            }
            let mut b = Vec::with_capacity(n as usize);
            while b.len() < n as usize {
                let r = it.next_rand().to_le_bytes();
                let take = (n as usize - b.len()).min(8);
                b.extend_from_slice(&r[..take]);
            }
            Ok(bytes_val(b))
        }

        // ═══ io ══════════════════════════════════════════════════════════
        "io.write" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let raw = b.borrow().clone();
            it.stdout.extend_from_slice(&raw);
            Ok(Value::Unit)
        }
        "io.write_bytes" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut b = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                b.push(want_int(name, x)? as u8);
            }
            it.stdout.extend_from_slice(&b);
            Ok(Value::Unit)
        }
        "io.read_n_bytes" => {
            // ref stdin is empty: every count agrees at 0 (count_domain_nonbytes)
            arity(name, &args, 1)?;
            let _ = want_int(name, &args[0])?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "io.read_all" | "io.read_line" => {
            arity(name, &args, 0)?;
            Ok(Value::str(""))
        }

        // ═══ fs leftovers ════════════════════════════════════════════════
        "fs.read_bytes_raw" => {
            arity(name, &args, 1)?;
            Ok(match crate::fsread::read(want_str(name, &args[0])?) {
                Ok(b) => ok(bytes_val(b)),
                Err(e) => io_err(e),
            })
        }
        "fs.write_bytes_raw" => {
            arity(name, &args, 2)?;
            let p = want_str(name, &args[0])?.to_string();
            let b = want_bytes(name, &args[1])?;
            let raw = b.borrow().clone();
            Ok(match std::fs::write(&p, &raw) {
                Ok(()) => ok(Value::Unit),
                Err(e) => io_err(e),
            })
        }
        "fs.read_lines_if_exists" | "fs.read_bytes_if_exists" | "fs.read_bytes_raw_if_exists" => {
            arity(name, &args, 1)?;
            let p = want_str(name, &args[0])?.to_string();
            if !std::path::Path::new(&p).exists() {
                return Ok(Ok(ok(Value::None)));
            }
            Ok(match name {
                "fs.read_lines_if_exists" => match crate::fsread::read_to_string(&p) {
                    Ok(s) => ok(some(Value::List(Rc::new(
                        crate::stdlib_ext::split_lines(&s)
                            .into_iter()
                            .map(|l| Value::str(&l))
                            .collect(),
                    )))),
                    Err(e) => io_err(e),
                },
                "fs.read_bytes_if_exists" => match crate::fsread::read(&p) {
                    Ok(b) => ok(some(Value::List(Rc::new(
                        b.into_iter().map(|x| Value::Int(x as i64)).collect(),
                    )))),
                    Err(e) => io_err(e),
                },
                _ => match crate::fsread::read(&p) {
                    Ok(b) => ok(some(bytes_val(b))),
                    Err(e) => io_err(e),
                },
            })
        }
        "fs.modified_at" => {
            arity(name, &args, 1)?;
            Ok(
                match std::fs::metadata(&**want_str(name, &args[0])?).and_then(|m| m.modified()) {
                    Ok(t) => ok(Value::Int(
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0),
                    )),
                    Err(e) => io_err(e),
                },
            )
        }
        "fs.walk" => {
            arity(name, &args, 1)?;
            let root = want_str(name, &args[0])?.to_string();
            let mut out: Vec<String> = Vec::new();
            match walk_dir(&root, &mut out) {
                Ok(()) => Ok(ok(Value::List(Rc::new(
                    out.into_iter().map(|p| Value::str(&p)).collect(),
                )))),
                Err(e) => Ok(io_err(e)),
            }
        }

        // ═══ bytes leftovers (RefCell reference semantics) ═══════════════
        "bytes.pad_left" | "bytes.pad_right" => {
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let n = want_int(name, &args[1])?;
            let fill = want_int(name, &args[2])? as u8;
            if n <= b.len() as i64 {
                return Ok(Ok(bytes_val(b.clone())));
            }
            if n > (1i64 << 31) {
                return Ok(Err(Flow::Abort("out of memory".into())));
            }
            let pad = n as usize - b.len();
            let mut v = Vec::with_capacity(n as usize);
            if name == "bytes.pad_left" {
                v.extend(std::iter::repeat_n(fill, pad));
                v.extend_from_slice(&b);
            } else {
                v.extend_from_slice(&b);
                v.extend(std::iter::repeat_n(fill, pad));
            }
            Ok(bytes_val(v))
        }
        "bytes.chunks" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let n = want_int(name, &args[1])?;
            if n <= 0 {
                return Ok(Err(Flow::Abort("chunk size must be positive".into())));
            }
            let n = (n as u64).min(b.len().max(1) as u64) as usize;
            let mut out: Vec<Value> = Vec::new();
            let mut i = 0;
            while i < b.len() {
                let e = (i + n).min(b.len());
                out.push(bytes_val(b[i..e].to_vec()));
                i = e;
            }
            Ok(Value::List(Rc::new(out)))
        }
        "bytes.copy_within" => {
            // in-place, `(b, src_start, src_end, dst)` — the third argument is
            // the source END, not a count. C-213: `src_end` CLAMPS to the
            // length, and the move happens only when the source range is
            // non-empty AND the destination window fits; any other window —
            // past the end or a NEGATIVE offset — is a no-op, never a clamp
            // (bytes_writer_family: copy_within_no_fit).
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let (src_start, src_end, dst) = (
                want_int(name, &args[1])?,
                want_int(name, &args[2])?,
                want_int(name, &args[3])?,
            );
            let mut v = b.borrow_mut();
            let len = v.len() as i64;
            // A negative `src_end` is enormous as the implementations'
            // `as usize` and saturates to the length, so it means "to the end".
            let end = if src_end < 0 || src_end > len {
                len
            } else {
                src_end
            };
            // Under the two tests before it, `end - src_start` is in
            // `[1, len]`, so `len - (end - src_start)` is in `[0, len)` and the
            // subtraction cannot wrap the way `dst + count <= len` did.
            if src_start >= 0 && src_start < end && dst >= 0 && dst <= len - (end - src_start) {
                let (s, e, d) = (src_start as usize, end as usize, dst as usize);
                v.copy_within(s..e, d);
            }
            Ok(Value::Unit)
        }
        // cursor writers: append at the end, in place (bytes_writer_family)
        "bytes.write_u8" | "bytes.write_bool" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let x = match (&args[1], name) {
                (Value::Bool(v), "bytes.write_bool") => *v as u8,
                (v, _) => want_int(name, v)? as u8,
            };
            b.borrow_mut().push(x);
            Ok(Value::Unit)
        }
        "bytes.write_u16_le" | "bytes.write_u16_be" | "bytes.write_u32_le"
        | "bytes.write_u32_be" | "bytes.write_i32_le" | "bytes.write_i32_be"
        | "bytes.write_i64_le" | "bytes.write_i64_be" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let raw = want_int(name, &args[1])? as u64;
            let width = if name.contains("16") {
                2
            } else if name.contains("32") {
                4
            } else {
                8
            };
            push_int(&mut b.borrow_mut(), raw, width, name.ends_with("be"));
            Ok(Value::Unit)
        }
        "bytes.write_f32_le" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            push_int(
                &mut b.borrow_mut(),
                obs_bits_f32(want_float(name, &args[1])? as f32) as u64,
                4,
                false,
            );
            Ok(Value::Unit)
        }
        "bytes.write_f64_be" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            push_int(
                &mut b.borrow_mut(),
                obs_bits_f64(want_float(name, &args[1])?),
                8,
                true,
            );
            Ok(Value::Unit)
        }
        "bytes.write_string_be" => {
            // u32 BE length prefix + UTF-8 bytes (bytes_writer_family cursor_utf8)
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let s = want_str(name, &args[1])?;
            let mut v = b.borrow_mut();
            push_int(&mut v, s.len() as u64, 4, true);
            v.extend_from_slice(s.as_bytes());
            Ok(Value::Unit)
        }

        // ═══ http offline builders (http_response_headers) ═══════════════
        "http.response" | "http.json" => {
            arity(name, &args, 2)?;
            let status = want_int(name, &args[0])?;
            let body = want_str(name, &args[1])?.clone();
            let ct = if name == "http.json" {
                "application/json"
            } else {
                "text/plain"
            };
            Ok(http_resp(
                status,
                &body,
                vec![("Content-Type".into(), ct.into())],
            ))
        }
        // ALS-R5: a request built in code; query_params reads the target
        // after its FIRST `?` by url.parse_query's rules.
        "http.new_request" => {
            arity(name, &args, 4)?;
            let method = want_str(name, &args[0])?.to_string();
            let target = want_str(name, &args[1])?.to_string();
            let body = want_str(name, &args[2])?.to_string();
            Ok(Value::Record {
                type_name: Some(Rc::from(crate::value::HTTP_REQUEST_TYPE)),
                fields: Rc::new(vec![
                    (Rc::from("method"), Value::str(&method)),
                    (Rc::from("path"), Value::str(&target)),
                    (Rc::from("body"), Value::str(&body)),
                    (Rc::from("headers"), args[3].clone()),
                ]),
            })
        }
        "http.query_params" => {
            arity(name, &args, 1)?;
            let target = match &args[0] {
                Value::Record {
                    type_name: Some(tn),
                    fields,
                } if &**tn == crate::value::HTTP_REQUEST_TYPE => fields
                    .iter()
                    .find(|(k, _)| &**k == "path")
                    .and_then(|(_, v)| match v {
                        Value::Str(s) => Some(s.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default(),
                other => return mismatch(name, "HttpRequest", other),
            };
            let cs: Vec<char> = target.chars().collect();
            let (_, query, _) = split_once_char(&cs, '?');
            Ok(
                match url_parse_query(&str_of(&query), "http.query_params") {
                    Ok(ps) => Value::Ok(Rc::new(pairs_value(ps))),
                    Err(msg) => Value::Err(Rc::new(Value::str(&msg))),
                },
            )
        }
        "http.redirect" => {
            arity(name, &args, 1)?;
            let loc = want_str(name, &args[0])?.to_string();
            Ok(http_resp(302, "", vec![("Location".into(), loc)]))
        }
        "http.with_headers" => {
            arity(name, &args, 3)?;
            let status = want_int(name, &args[0])?;
            let body = want_str(name, &args[1])?.clone();
            let hs = match &args[2] {
                Value::Map(m) => {
                    let mut v: Vec<(String, String)> = Vec::new();
                    for (k, val) in m.iter() {
                        v.push((
                            want_str(name, k)?.to_string(),
                            want_str(name, val)?.to_string(),
                        ));
                    }
                    v
                }
                other => return mismatch(name, "Map[String, String]", other),
            };
            Ok(http_resp(status, &body, hs))
        }
        "http.get_header" => {
            arity(name, &args, 2)?;
            let key = want_str(name, &args[1])?.to_string();
            let hs = http_headers(name, &args[0])?;
            Ok(
                match hs.iter().rev().find(|(k, _)| eq_ignore_ascii(k, &key)) {
                    Some((_, v)) => some(Value::str(v)),
                    None => Value::None,
                },
            )
        }
        "http.set_header" => {
            arity(name, &args, 3)?;
            let key = want_str(name, &args[1])?.to_string();
            let val = want_str(name, &args[2])?.to_string();
            let (status, body, mut hs) = http_parts(name, &args[0])?;
            match hs.iter_mut().find(|(k, _)| eq_ignore_ascii(k, &key)) {
                Some(slot) => slot.1 = val,
                None => hs.push((key, val)),
            }
            Ok(http_resp(status, &body, hs))
        }
        "http.status" => {
            arity(name, &args, 2)?;
            let (_, body, hs) = http_parts(name, &args[0])?;
            Ok(http_resp(want_int(name, &args[1])?, &body, hs))
        }
        "http.body" => {
            arity(name, &args, 1)?;
            let (_, body, _) = http_parts(name, &args[0])?;
            Ok(Value::str(&body))
        }

        // ═══ string / list / result / map leftovers ══════════════════════
        "string.run_length_encode" => {
            arity(name, &args, 1)?;
            let cs: Vec<char> = want_str(name, &args[0])?.chars().collect();
            let mut out: Vec<Value> = Vec::new();
            let mut i = 0;
            while i < cs.len() {
                let mut j = i + 1;
                while j < cs.len() && cs[j] == cs[i] {
                    j += 1;
                }
                out.push(Value::Tuple(Rc::new(vec![
                    Value::str(&cs[i].to_string()),
                    Value::Int((j - i) as i64),
                ])));
                i = j;
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.length" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_list(name, &args[0])?.len() as i64))
        }
        "list.tail" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?;
            Ok(Value::List(Rc::new(if xs.is_empty() {
                Vec::new()
            } else {
                xs[1..].to_vec()
            })))
        }
        "list.with_capacity" => {
            // a capacity HINT: allocates nothing observable (list_with_capacity
            // holds 2^31-1 without memory movement)
            arity(name, &args, 1)?;
            let _ = want_int(name, &args[0])?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "list.window" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let n = want_int(name, &args[1])?;
            if n <= 0 {
                return Ok(Err(Flow::Abort("window size must be positive".into())));
            }
            let n = n as usize;
            let mut out: Vec<Value> = Vec::new();
            if n <= xs.len() {
                for i in 0..=(xs.len() - n) {
                    out.push(Value::List(Rc::new(xs[i..i + n].to_vec())));
                }
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.binary_search" => {
            // pinned by binary_search_duplicate_keys: the LAST equal index
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let key = &args[1];
            let mut lo: i64 = 0;
            let mut hi: i64 = xs.len() as i64 - 1;
            let mut found: Option<i64> = None;
            while lo <= hi {
                let mid = (lo + hi) / 2;
                match crate::value::value_cmp(&xs[mid as usize], key) {
                    Some(std::cmp::Ordering::Less) => lo = mid + 1,
                    Some(std::cmp::Ordering::Greater) => hi = mid - 1,
                    Some(std::cmp::Ordering::Equal) => {
                        found = Some(mid);
                        lo = mid + 1; // keep looking right: last equal wins
                    }
                    None => return mismatch(name, "comparable elements", key),
                }
            }
            Ok(match found {
                Some(i) => some(Value::Int(i)),
                None => Value::None,
            })
        }
        "result.flatten" => {
            arity(name, &args, 1)?;
            Ok(match &args[0] {
                Value::Ok(inner) => (**inner).clone(),
                Value::Err(e) => Value::Err(e.clone()),
                other => return mismatch(name, "Result", other),
            })
        }
        "result.or_else" => {
            arity(name, &args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Ok(_), _) => Ok(args[0].clone()),
                (Value::Err(e), Value::Fn(c)) => {
                    let c = c.clone();
                    let e = (**e).clone();
                    Ok(it.call_value(&c, vec![e])?)
                }
                (other, _) => mismatch(name, "Result", other),
            }
        }
        "map.upsert" => {
            arity(name, &args, 4)?;
            let m = match &args[0] {
                Value::Map(m) => m.clone(),
                other => return mismatch(name, "Map", other),
            };
            let key = args[1].clone();
            let c = match &args[3] {
                Value::Fn(c) => c.clone(),
                other => return mismatch(name, "a function", other),
            };
            let mut out: Vec<(Value, Value)> = (*m).clone();
            let mut hit = false;
            for slot in out.iter_mut() {
                if crate::value::values_eq(&slot.0, &key) == Some(true) {
                    slot.1 = it.call_value(&c, vec![slot.1.clone()])?;
                    hit = true;
                    break;
                }
            }
            if !hit {
                out.push((key, args[2].clone()));
            }
            Ok(Value::Map(Rc::new(out)))
        }

        // ═══ fan mapper forms (ALS-R3 determinism: sequential = parallel) ═══
        "fan.map" | "fan.settle" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = match &args[1] {
                Value::Fn(c) => c.clone(),
                other => return mismatch(name, "a function", other),
            };
            let mut vals: Vec<Value> = Vec::new();
            // C-005 / ADR-0024 D1: fan.map evaluates EVERY element, and the
            // lowest-index Err is the one that surfaces
            let mut first_err: Option<Rc<Value>> = None;
            for x in xs.iter() {
                let r = it.call_value(&c, vec![x.clone()])?;
                match (name, r) {
                    ("fan.map", Value::Ok(v)) => vals.push((*v).clone()),
                    ("fan.map", Value::Err(e)) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                    ("fan.map", v) => vals.push(v),
                    ("fan.settle", Value::Ok(v)) => vals.push(Value::Ok(v)),
                    ("fan.settle", Value::Err(e)) => vals.push(Value::Err(e)),
                    ("fan.settle", v) => vals.push(Value::Ok(Rc::new(v))),
                    _ => unreachable!(),
                }
            }
            Ok(match (name, first_err) {
                ("fan.map", Some(e)) => Value::Err(e),
                ("fan.map", None) => ok(Value::List(Rc::new(vals))),
                _ => Value::List(Rc::new(vals)),
            })
        }

        _ => return Err(Flow::Fatal("__not_ext__".into())),
    })
}

/// The day number of a timestamp, FLOORED (ALS-T26, C-359): a pre-epoch second
/// off a day boundary belongs to the day BEFORE the one truncation names.
fn days_from_epoch(ts: i64) -> i64 {
    ts.div_euclid(86400)
}

/// civil_from_days translated from stdlib/datetime_calendar.almd
/// (floor division for the day number, wrapping arithmetic) → (year, month, day)
fn civ(ts: i64) -> (i64, i64, i64) {
    let z = days_from_epoch(ts) + 719468;
    let zadj = if z >= 0 { z } else { z.wrapping_sub(146096) };
    let era = zadj / 146097;
    let doe = z.wrapping_sub(era.wrapping_mul(146097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (yoe.wrapping_mul(365) + yoe / 4 - yoe / 100);
    let mp = (doy.wrapping_mul(5).wrapping_add(2)) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let d = doy - (mp.wrapping_mul(153).wrapping_add(2)) / 5 + 1;
    let y0 = yoe.wrapping_add(era.wrapping_mul(400));
    let y = if m <= 2 { y0 + 1 } else { y0 };
    (y, m, d)
}

/// __iso_pad translated literally: right-to-left digits via n%10 / n/10
/// (truncating — replicates the negative-input behavior byte for byte)
/// ASCII digits only, non-empty — `string.is_digit`'s rule (ALS-T26/T27).
/// Written over chars: ADR-0015 clause 5 forbids leaning on Rust's own
/// splitting / parsing / trimming, so agreement with a target measures the
/// ALS text rather than a shared std.
fn all_digits(cs: &[char]) -> bool {
    !cs.is_empty() && cs.iter().all(|c| c.is_ascii_digit())
}

/// `sign? digit+` over chars, in the ALS-T8 grammar's digit-accumulating shape.
fn digits_to_i64(cs: &[char]) -> Option<i64> {
    if !all_digits(cs) {
        return None;
    }
    let mut acc: i64 = 0;
    for c in cs {
        let d = (*c as i64) - ('0' as i64);
        acc = acc.checked_mul(10)?.checked_add(d)?;
    }
    Some(acc)
}

/// Split on a single char, every field kept (the `string.split` shape).
fn split_char(cs: &[char], sep: char) -> Vec<Vec<char>> {
    let mut out: Vec<Vec<char>> = vec![Vec::new()];
    for c in cs {
        if *c == sep {
            out.push(Vec::new());
        } else {
            out.last_mut().expect("one field always exists").push(*c);
        }
    }
    out
}

/// Split at the FIRST occurrence of `sep`: (before, after, found).
fn split_once_char(cs: &[char], sep: char) -> (Vec<char>, Vec<char>, bool) {
    match cs.iter().position(|c| *c == sep) {
        Some(i) => (cs[..i].to_vec(), cs[i + 1..].to_vec(), true),
        None => (cs.to_vec(), Vec::new(), false),
    }
}

fn str_of(cs: &[char]) -> String {
    cs.iter().collect()
}

/// Every field of `half` as a number, or None at the first refused field.
fn iso_fields(half: &[char], sep: char) -> Option<Vec<i64>> {
    split_char(half, sep)
        .iter()
        .map(|p| digits_to_i64(p))
        .collect()
}

fn iso_days_in_month(y: i64, m: i64) -> i64 {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    match m {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// epoch_from_civil — the inverse of `civ`, the shape datetime.from_parts has.
fn iso_epoch(y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    let ya = if m <= 2 { y - 1 } else { y };
    let era = if ya >= 0 { ya } else { ya - 399 } / 400;
    let yoe = ya - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) * 86400 + h * 3600 + mi * 60 + s
}

/// ALS-T26's grammar, over chars. The ALS whitespace set does the trimming.
fn iso_parse(raw: &str) -> Result<i64, String> {
    let all: Vec<char> = raw.chars().collect();
    let mut lo = 0;
    let mut hi = all.len();
    while lo < hi && crate::stdlib::is_als_whitespace(all[lo]) {
        lo += 1;
    }
    while hi > lo && crate::stdlib::is_als_whitespace(all[hi - 1]) {
        hi -= 1;
    }
    let t = &all[lo..hi];
    let halves = split_char(t, 'T');
    if halves.len() != 2 {
        return Err("expected YYYY-MM-DDTHH:MM:SSZ".to_string());
    }
    let date = &halves[0];
    let time_raw = &halves[1];
    let had_z = time_raw.last() == Some(&'Z');
    let time_z: Vec<char> = if had_z {
        time_raw[..time_raw.len() - 1].to_vec()
    } else {
        time_raw.clone()
    };
    let sign_at = time_z.iter().position(|c| *c == '+' || *c == '-');
    if had_z && sign_at.is_some() {
        return Err("invalid datetime format".to_string());
    }
    let hms: Vec<char> = match sign_at {
        Some(i) => time_z[..i].to_vec(),
        None => time_z.clone(),
    };
    let (Some(d), Some(tm)) = (iso_fields(date, '-'), iso_fields(&hms, ':')) else {
        return Err("invalid datetime format".to_string());
    };
    if d.len() != 3 || tm.len() != 3 {
        return Err("invalid datetime format".to_string());
    }
    let offset = match sign_at {
        None => 0,
        Some(i) => {
            let raw_off = str_of(&time_z[i..]);
            match iso_fields(&time_z[i + 1..], ':').as_deref() {
                Some([oh, om]) if (0..=23).contains(oh) && (0..=59).contains(om) => {
                    let secs = oh * 3600 + om * 60;
                    if time_z[i] == '-' {
                        -secs
                    } else {
                        secs
                    }
                }
                _ => {
                    return Err(format!(
                        "invalid offset: {}",
                        crate::stdlib_ext::err_escape(&raw_off)
                    ))
                }
            }
        }
    };
    Ok(iso_civil(d[0], d[1], d[2], tm[0], tm[1], tm[2])? - offset)
}

/// The one range check of both constructors (parse_iso, from_parts): the
/// first field out of range, by name, else the UTC instant.
fn iso_civil(y: i64, mo: i64, dd: i64, h: i64, mi: i64, sec: i64) -> Result<i64, String> {
    // RFC 3339's four-digit year; checked first, so no later step can wrap.
    if !(0..=9999).contains(&y) {
        return Err(format!("year out of range: {y}"));
    }
    if !(1..=12).contains(&mo) {
        return Err(format!("month out of range: {mo}"));
    }
    if dd < 1 || dd > iso_days_in_month(y, mo) {
        return Err(format!("day out of range: {dd}"));
    }
    if !(0..=23).contains(&h) {
        return Err(format!("hour out of range: {h}"));
    }
    if !(0..=59).contains(&mi) {
        return Err(format!("minute out of range: {mi}"));
    }
    if !(0..=59).contains(&sec) {
        return Err(format!("second out of range: {sec}"));
    }
    Ok(iso_epoch(y, mo, dd, h, mi, sec))
}

/// RFC 3986 reg-name without pct-encoded: unreserved / sub-delims (ALS-T27).
fn url_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '.'
                | '_'
                | '~'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
        )
}

fn url_record(
    scheme: &str,
    host: &str,
    port: Option<i64>,
    path: &str,
    query: &str,
    fragment: &str,
) -> Value {
    Value::Record {
        type_name: Some(Rc::from("Url")),
        fields: Rc::new(vec![
            (Rc::from("scheme"), Value::str(scheme)),
            (Rc::from("host"), Value::str(host)),
            (
                Rc::from("port"),
                match port {
                    Some(p) => Value::Some(Rc::new(Value::Int(p))),
                    None => Value::None,
                },
            ),
            (Rc::from("path"), Value::str(path)),
            (Rc::from("query"), Value::str(query)),
            (Rc::from("fragment"), Value::str(fragment)),
        ]),
    }
}

type UrlParts = (String, String, Option<i64>, String, String, String);

fn url_parts(name: &str, v: &Value) -> Result<UrlParts, Flow> {
    match v {
        Value::Record {
            type_name: Some(tn),
            fields,
        } if &**tn == "Url" => {
            let mut out: UrlParts = (
                String::new(),
                String::new(),
                None,
                String::new(),
                String::new(),
                String::new(),
            );
            for (k, val) in fields.iter() {
                match (&**k, val) {
                    ("scheme", Value::Str(s)) => out.0 = s.to_string(),
                    ("host", Value::Str(s)) => out.1 = s.to_string(),
                    ("port", Value::Some(p)) => {
                        if let Value::Int(n) = &**p {
                            out.2 = Some(*n);
                        }
                    }
                    ("port", Value::None) => out.2 = None,
                    ("path", Value::Str(s)) => out.3 = s.to_string(),
                    ("query", Value::Str(s)) => out.4 = s.to_string(),
                    ("fragment", Value::Str(s)) => out.5 = s.to_string(),
                    _ => {}
                }
            }
            Ok(out)
        }
        _ => Err(Flow::Fatal(format!("{name}: expected a Url record"))),
    }
}

/// ALS-T27, over chars: the input is cleaned as the WHATWG URL parser cleans
/// it (leading and trailing C0 control or space stripped, every tab, LF and CR
/// removed), then a remaining control character or a backslash is refused.
fn url_clean(raw: &str, who: &str) -> Result<Vec<char>, String> {
    let cs: Vec<char> = raw.chars().collect();
    let c0_or_space = |c: &char| (*c as u32) <= 0x20;
    let start = cs.iter().position(|c| !c0_or_space(c)).unwrap_or(cs.len());
    let end = cs
        .iter()
        .rposition(|c| !c0_or_space(c))
        .map_or(start, |i| i + 1);
    let kept: Vec<char> = cs[start..end]
        .iter()
        .copied()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    if kept.iter().any(|c| (*c as u32) < 0x20 || *c as u32 == 0x7f) {
        return Err(format!("{who}: control character in URL"));
    }
    if kept.contains(&'\\') {
        return Err(format!("{who}: backslash in URL"));
    }
    Ok(kept)
}

/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn url_scheme_ok(cs: &[char]) -> bool {
    !cs.is_empty()
        && cs[0].is_ascii_alphabetic()
        && cs
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn url_ascii_lower(cs: &[char]) -> String {
    cs.iter()
        .map(|c| {
            if c.is_ascii_uppercase() {
                char::from_u32(*c as u32 + 32).unwrap_or(*c)
            } else {
                *c
            }
        })
        .collect()
}

/// WHATWG's ends-in-a-number test: the last label (a single trailing empty
/// label dropped) is all ASCII digits, or `0x`/`0X` and hex digits.
fn url_ends_in_number(host: &[char]) -> bool {
    let mut labels = split_char(host, '.');
    if labels.len() > 1 && labels.last().is_some_and(|l| l.is_empty()) {
        labels.pop();
    }
    let last = labels.last().cloned().unwrap_or_default();
    if all_digits(&last) {
        return true;
    }
    last.len() >= 2
        && last[0] == '0'
        && (last[1] == 'x' || last[1] == 'X')
        && last[2..].iter().all(|c| c.is_ascii_hexdigit())
}

/// Four decimal labels, each `0` or a non-zero-led number up to 255.
fn url_canonical_ipv4(host: &[char]) -> bool {
    let labels = split_char(host, '.');
    labels.len() == 4
        && labels.iter().all(|l| {
            all_digits(l)
                && l.len() <= 3
                && (l.len() == 1 || l[0] != '0')
                && digits_to_i64(l).is_some_and(|n| n <= 255)
        })
}

/// The dot-segment rule (WHATWG path state): `.` and `%2e` (any case) are a
/// single-dot segment; `..`, `.%2e`, `%2e.` and `%2e%2e` are a double-dot
/// segment. A double-dot drops the segment before it; either kind, as the
/// last segment, leaves the path ending in `/`.
fn url_dot_kind(seg: &[char]) -> u8 {
    let lower = url_ascii_lower(seg);
    match lower.as_str() {
        "." | "%2e" => 1,
        ".." | ".%2e" | "%2e." | "%2e%2e" => 2,
        _ => 0,
    }
}

fn url_remove_dots(path: &str) -> String {
    let cs: Vec<char> = path.chars().collect();
    if cs.is_empty() {
        return String::new();
    }
    let rooted = cs[0] == '/';
    let body = if rooted { &cs[1..] } else { &cs[..] };
    let segs = split_char(body, '/');
    let n = segs.len();
    let mut out: Vec<Vec<char>> = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == n;
        match url_dot_kind(seg) {
            2 => {
                out.pop();
                if last {
                    out.push(Vec::new());
                }
            }
            1 => {
                if last {
                    out.push(Vec::new());
                }
            }
            _ => out.push(seg.clone()),
        }
    }
    let mut s = String::new();
    if rooted {
        s.push('/');
    }
    for (i, seg) in out.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(&str_of(seg));
    }
    s
}

/// ALS-T27, over chars.
fn url_parse(raw: &str) -> Result<Value, String> {
    let cs = url_clean(raw, "url.parse")?;
    let Some(colon) = cs.iter().position(|c| *c == ':') else {
        return Err("url.parse: missing '://' scheme separator".to_string());
    };
    if colon == 0 {
        return Err("url.parse: empty scheme".to_string());
    }
    if !url_scheme_ok(&cs[..colon]) {
        return Err(format!(
            "url.parse: invalid scheme: {}",
            str_of(&cs[..colon])
        ));
    }
    if cs.get(colon + 1) != Some(&'/') || cs.get(colon + 2) != Some(&'/') {
        return Err("url.parse: missing '://' scheme separator".to_string());
    }
    let scheme = url_ascii_lower(&cs[..colon]);
    let rest = cs[colon + 3..].to_vec();
    let (before_frag, fragment, _) = split_once_char(&rest, '#');
    let (before_query, query, _) = split_once_char(&before_frag, '?');
    let (authority, after_slash, had_slash) = split_once_char(&before_query, '/');
    let path = if had_slash {
        url_remove_dots(&format!("/{}", str_of(&after_slash)))
    } else {
        String::new()
    };
    let auth_text = str_of(&authority);
    if authority.contains(&'@') {
        return Err(format!("url.parse: userinfo is not supported: {auth_text}"));
    }
    if authority.first() == Some(&'[') {
        return Err(format!(
            "url.parse: IPv6 hosts are not supported: {auth_text}"
        ));
    }
    let (host, port_field, had_port) = split_once_char(&authority, ':');
    let host_text = str_of(&host);
    if host.is_empty() {
        return Err("url.parse: empty host".to_string());
    }
    if !host.iter().all(|c| url_host_char(*c)) {
        return Err(format!("url.parse: invalid host: {host_text}"));
    }
    if url_ends_in_number(&host) && !url_canonical_ipv4(&host) {
        return Err(format!("url.parse: non-canonical IPv4 host: {host_text}"));
    }
    let port_text = str_of(&port_field);
    let port = if had_port {
        match digits_to_i64(&port_field) {
            None => return Err(format!("url.parse: invalid port: {port_text}")),
            Some(p) if p <= 65535 => Some(p),
            Some(_) => return Err(format!("url.parse: port out of range: {port_text}")),
        }
    } else {
        None
    };
    Ok(url_record(
        &scheme,
        &url_ascii_lower(&host),
        port,
        &path,
        &str_of(&query),
        &str_of(&fragment),
    ))
}

/// ALS-T27 resolve (RFC 3986 §5.2 over the cleaned reference): a reference
/// with a scheme is parsed alone, `//` takes the base's scheme, anything
/// else replaces the base's path / query / fragment, every path through the
/// one dot-segment rule.
fn url_resolve(base: &Value, raw: &str) -> Result<Value, String> {
    let (scheme, host, port, bpath, bquery, _) =
        url_parts("url.resolve", base).map_err(|_| "url.resolve: expected a Url".to_string())?;
    let cs = url_clean(raw, "url.resolve")?;
    let colon = cs.iter().position(|c| *c == ':');
    if colon.is_some_and(|i| url_scheme_ok(&cs[..i])) {
        return url_parse(&str_of(&cs));
    }
    if cs.len() >= 2 && cs[0] == '/' && cs[1] == '/' {
        return url_parse(&format!("{scheme}:{}", str_of(&cs)));
    }
    let (before_frag, fragment, _) = split_once_char(&cs, '#');
    let (rpath, query, had_query) = split_once_char(&before_frag, '?');
    let path = if rpath.is_empty() {
        bpath.clone()
    } else if rpath[0] == '/' {
        url_remove_dots(&str_of(&rpath))
    } else {
        let merged = if bpath.is_empty() {
            format!("/{}", str_of(&rpath))
        } else {
            let bp: Vec<char> = bpath.chars().collect();
            let cut = bp.iter().rposition(|c| *c == '/').map_or(0, |i| i + 1);
            format!("{}{}", str_of(&bp[..cut]), str_of(&rpath))
        };
        url_remove_dots(&merged)
    };
    let query = if rpath.is_empty() && !had_query {
        bquery
    } else {
        str_of(&query)
    };
    Ok(url_record(
        &scheme,
        &host,
        port,
        &path,
        &query,
        &str_of(&fragment),
    ))
}

fn url_hex(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// url.decode_component: a run of `%XX` is strict UTF-8; a `%` without two
/// following chars is truncated, one without two hex digits is invalid.
fn url_decode_component(s: &str, who: &str) -> Result<String, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '%' {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        let mut run: Vec<u8> = Vec::new();
        while i < cs.len() && cs[i] == '%' {
            if i + 3 > cs.len() {
                return Err(format!("{who}: truncated percent escape"));
            }
            match (url_hex(cs[i + 1]), url_hex(cs[i + 2])) {
                (Some(h), Some(l)) => run.push(h * 16 + l),
                _ => return Err(format!("{who}: invalid percent escape")),
            }
            i += 3;
        }
        match std::str::from_utf8(&run) {
            Ok(t) => out.push_str(t),
            Err(_) => return Err(format!("{who}: invalid UTF-8 in percent data")),
        }
    }
    Ok(out)
}

/// url.parse_query: `&`-separated, empty segments dropped, each split at its
/// first `=` (none → value ""), both sides `+` → space then decoded.
fn url_parse_query(q: &str, who: &str) -> Result<Vec<(String, String)>, String> {
    let cs: Vec<char> = q.chars().collect();
    let mut out: Vec<(String, String)> = Vec::new();
    for part in split_char(&cs, '&') {
        if part.is_empty() {
            continue;
        }
        let (k, v, _) = split_once_char(&part, '=');
        let plus =
            |x: &[char]| -> String { x.iter().map(|c| if *c == '+' { ' ' } else { *c }).collect() };
        let k = url_decode_component(&plus(&k), who)?;
        let v = url_decode_component(&plus(&v), who)?;
        out.push((k, v));
    }
    Ok(out)
}

fn pairs_value(ps: Vec<(String, String)>) -> Value {
    Value::List(Rc::new(
        ps.into_iter()
            .map(|(k, v)| Value::Tuple(Rc::new(vec![Value::str(&k), Value::str(&v)])))
            .collect(),
    ))
}

/// Rust's `{:04}` for a year: at least four columns, zero-padded, the sign
/// occupying one of them. Shared by to_iso and format's %Y (ALS-T17).
fn iso_year(y: i64) -> String {
    let neg = y < 0;
    let mag = if neg { 0i64.wrapping_sub(y) } else { y };
    let width = if neg {
        dt_digits(mag).max(3)
    } else {
        dt_digits(mag).max(4)
    };
    let mut ystr = pad_num(mag, width);
    if neg {
        ystr.insert(0, '-');
    }
    ystr
}

fn pad_num(n: i64, width: usize) -> String {
    let mut buf = vec![b'0'; width];
    let mut v = n;
    for i in (0..width).rev() {
        let digit = v % 10;
        buf[i] = 48u8.wrapping_add(digit as u8);
        v /= 10;
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn dt_digits(n: i64) -> usize {
    if n < 10 {
        1
    } else {
        1 + dt_digits(n / 10)
    }
}

fn push_int(v: &mut Vec<u8>, raw: u64, width: usize, be: bool) {
    for j in 0..width {
        let shift = if be { 8 * (width - 1 - j) } else { 8 * j };
        v.push((raw >> shift) as u8);
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('?'));
        out.push(char::from_digit((b & 15) as u32, 16).unwrap_or('?'));
    }
    out
}

fn eq_ignore_ascii(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x.eq_ignore_ascii_case(&y))
}

const B64_STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(data: &[u8], url: bool) -> String {
    let table = if url { B64_URL } else { B64_STD };
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(table[(n >> 18) as usize & 63] as char);
        out.push(table[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            table[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            table[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// ALS-T11's canonical decode, the first failing rule wins: the UTF-8 byte
/// length ≢ 0 (mod 4) → "invalid base64 length: N"; more than two trailing
/// '=' → "invalid base64 padding"; any other byte outside THIS function's
/// table — a '=' before the trailing run, or the other alphabet's 62/63
/// pair — → "invalid base64 character"; a nonzero bit under the padding (the
/// low 2 bits of the last symbol for one '=', the low 4 for two) →
/// "invalid base64 trailing bits".
fn b64_decode(s: &str, url: bool) -> Result<Vec<u8>, String> {
    let table = if url { B64_URL } else { B64_STD };
    let bs = s.as_bytes();
    if !bs.len().is_multiple_of(4) {
        return Err(format!("invalid base64 length: {}", bs.len()));
    }
    let pad = bs.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 {
        return Err("invalid base64 padding".into());
    }
    let mut vals: Vec<u8> = Vec::new();
    for &c in &bs[..bs.len() - pad] {
        match table.iter().position(|&t| t == c) {
            Some(idx) => vals.push(idx as u8),
            None => return Err("invalid base64 character".into()),
        }
    }
    let under_pad: u8 = [0, 0b11, 0b1111][pad];
    if vals.last().is_some_and(|v| v & under_pad != 0) {
        return Err("invalid base64 trailing bits".into());
    }
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let n = ((chunk[0] as u32) << 18)
            | ((*chunk.get(1).unwrap_or(&0) as u32) << 12)
            | ((*chunk.get(2).unwrap_or(&0) as u32) << 6)
            | *chunk.get(3).unwrap_or(&0) as u32;
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

fn walk_dir(dir: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let rd = std::fs::read_dir(dir)?;
    let mut names: Vec<String> = Vec::new();
    for e in rd.flatten() {
        names.push(e.file_name().to_string_lossy().to_string());
    }
    // byte-lexicographic order without the host sort (ALS-R6 doctrine)
    let mut sorted: Vec<String> = Vec::with_capacity(names.len());
    for n in names {
        let pos = sorted
            .iter()
            .position(|m| m.as_bytes() > n.as_bytes())
            .unwrap_or(sorted.len());
        sorted.insert(pos, n);
    }
    for n in sorted {
        let full = format!("{dir}/{n}");
        out.push(full.clone());
        if std::path::Path::new(&full).is_dir() {
            walk_dir(&full, out)?;
        }
    }
    Ok(())
}

fn http_resp(status: i64, body: &str, headers: Vec<(String, String)>) -> Value {
    let hs: Vec<(Value, Value)> = headers
        .into_iter()
        .map(|(k, v)| (Value::str(&k), Value::str(&v)))
        .collect();
    Value::Record {
        type_name: Some(Rc::from(crate::value::HTTP_RESPONSE_TYPE)),
        fields: Rc::new(vec![
            (Rc::from("status"), Value::Int(status)),
            (Rc::from("body"), Value::str(body)),
            (Rc::from("headers"), Value::Map(Rc::new(hs))),
        ]),
    }
}

type HttpParts = (i64, String, Vec<(String, String)>);

fn http_parts(name: &str, v: &Value) -> Result<HttpParts, Flow> {
    match v {
        Value::Record {
            type_name: Some(tn),
            fields,
        } if &**tn == crate::value::HTTP_RESPONSE_TYPE => {
            let mut status = 0i64;
            let mut body = String::new();
            let mut hs: Vec<(String, String)> = Vec::new();
            for (k, val) in fields.iter() {
                match (&**k, val) {
                    ("status", Value::Int(n)) => status = *n,
                    ("body", Value::Str(s)) => body = s.to_string(),
                    ("headers", Value::Map(m)) => {
                        for (hk, hv) in m.iter() {
                            if let (Value::Str(a), Value::Str(b)) = (hk, hv) {
                                hs.push((a.to_string(), b.to_string()));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok((status, body, hs))
        }
        other => mismatch(name, "HttpResponse", other),
    }
}

fn http_headers(name: &str, v: &Value) -> Result<Vec<(String, String)>, Flow> {
    Ok(http_parts(name, v)?.2)
}

/// SHA-256 (FIPS 180-4), textbook implementation
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// SHA-512 (FIPS 180-4), textbook implementation
fn sha512(data: &[u8]) -> [u8; 64] {
    const K: [u64; 80] = [
        0x428a2f98d728ae22,
        0x7137449123ef65cd,
        0xb5c0fbcfec4d3b2f,
        0xe9b5dba58189dbbc,
        0x3956c25bf348b538,
        0x59f111f1b605d019,
        0x923f82a4af194f9b,
        0xab1c5ed5da6d8118,
        0xd807aa98a3030242,
        0x12835b0145706fbe,
        0x243185be4ee4b28c,
        0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f,
        0x80deb1fe3b1696b1,
        0x9bdc06a725c71235,
        0xc19bf174cf692694,
        0xe49b69c19ef14ad2,
        0xefbe4786384f25e3,
        0x0fc19dc68b8cd5b5,
        0x240ca1cc77ac9c65,
        0x2de92c6f592b0275,
        0x4a7484aa6ea6e483,
        0x5cb0a9dcbd41fbd4,
        0x76f988da831153b5,
        0x983e5152ee66dfab,
        0xa831c66d2db43210,
        0xb00327c898fb213f,
        0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2,
        0xd5a79147930aa725,
        0x06ca6351e003826f,
        0x142929670a0e6e70,
        0x27b70a8546d22ffc,
        0x2e1b21385c26c926,
        0x4d2c6dfc5ac42aed,
        0x53380d139d95b3df,
        0x650a73548baf63de,
        0x766a0abb3c77b2a8,
        0x81c2c92e47edaee6,
        0x92722c851482353b,
        0xa2bfe8a14cf10364,
        0xa81a664bbc423001,
        0xc24b8b70d0f89791,
        0xc76c51a30654be30,
        0xd192e819d6ef5218,
        0xd69906245565a910,
        0xf40e35855771202a,
        0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8,
        0x1e376c085141ab53,
        0x2748774cdf8eeb99,
        0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63,
        0x4ed8aa4ae3418acb,
        0x5b9cca4f7763e373,
        0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc,
        0x78a5636f43172f60,
        0x84c87814a1f0ab72,
        0x8cc702081a6439ec,
        0x90befffa23631e28,
        0xa4506cebde82bde9,
        0xbef9a3f7b2c67915,
        0xc67178f2e372532b,
        0xca273eceea26619c,
        0xd186b8c721c0c207,
        0xeada7dd6cde0eb1e,
        0xf57d4f7fee6ed178,
        0x06f067aa72176fba,
        0x0a637dc5a2c898a6,
        0x113f9804bef90dae,
        0x1b710b35131c471b,
        0x28db77f523047d84,
        0x32caab7b40c72493,
        0x3c9ebe0a15c9bebc,
        0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6,
        0x597f299cfc657e2a,
        0x5fcb6fab3ad6faec,
        0x6c44198c4a475817,
    ];
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u128).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for block in msg.chunks(128) {
        let mut w = [0u64; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let mut b8 = [0u8; 8];
            b8.copy_from_slice(&block[i * 8..i * 8 + 8]);
            *word = u64::from_be_bytes(b8);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *hi = hi.wrapping_add(v);
        }
    }
    let mut out = [0u8; 64];
    for (i, word) in h.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// HMAC (RFC 2104) over SHA-256 (block 64) or SHA-512 (block 128).
fn hmac(wide: bool, key: &[u8], msg: &[u8]) -> Vec<u8> {
    let digest = |d: &[u8]| -> Vec<u8> {
        if wide {
            sha512(d).to_vec()
        } else {
            sha256(d).to_vec()
        }
    };
    let block = if wide { 128 } else { 64 };
    let mut k = if key.len() > block {
        digest(key)
    } else {
        key.to_vec()
    };
    k.resize(block, 0);
    let mut inner: Vec<u8> = k.iter().map(|x| x ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let mut outer: Vec<u8> = k.iter().map(|x| x ^ 0x5c).collect();
    outer.extend_from_slice(&digest(&inner));
    digest(&outer)
}

/// The Lanczos core of `math.log_gamma` (g=7, n=9), valid for x >= 0.5:
/// stdlib/math_lgamma.almd's `__lgamma_lanczos` op for op, with the exact bit
/// constants and both `ln`s through the vendored libm log.
fn lanczos_log_gamma(x0: f64) -> f64 {
    let x = x0 - 1.0;
    let c: [f64; 9] = [
        f64::from_bits(4607182418800015696),
        f64::from_bits(4649161951908399877),
        f64::from_bits((-4570119468569323749i64) as u64),
        f64::from_bits(4649995848448718040),
        f64::from_bits((-4582953931388014755i64) as u64),
        f64::from_bits(4623230626370919553),
        f64::from_bits((-4629211466700216235i64) as u64),
        f64::from_bits(4532011357038326351),
        f64::from_bits(4504784147394309871),
    ];
    let mut ag = c[0];
    for (i, ci) in c.iter().enumerate().skip(1) {
        ag += ci / (x + i as f64);
    }
    let t = x + 7.5;
    let half_ln_2pi = f64::from_bits(4606452282016710324);
    let lt = libm::almide_rt_libm_log(t);
    let lag = libm::almide_rt_libm_log(ag);
    half_ln_2pi + (x + 0.5) * lt - t + lag
}
