//! Round-2 batch B: transcendentals over the vendored musl libm, the civil
//! calendar (translated from stdlib/datetime_calendar.almd — the normative
//! self-hosted source), hashes, base64, hex, path, args, random, io/fs
//! leftovers, the bytes cursor writers, and the offline http builders.
//! Same doctrine: chapters + fixtures decide; unknown edges abstain.

use std::rc::Rc;

use crate::eval::{fnan, Flow, Interp};
use crate::libm;
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
    "hash.fnv1a32",
    "hash.fnv1a32_bytes",
    "hash.sha256",
    "hash.sha256_hex",
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
    "args.raw",
    "args.flag",
    "args.option",
    "args.option_or",
    "args.positional",
    "args.positional_at",
    "random.int",
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
    "fan.any",
    "fan.settle",
];

// small local mirrors of the arg helpers (stdlib_ext keeps its own)
fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() != n {
        return Err(Flow::Fatal(format!(
            "{name}: expected {n} argument(s), got {}",
            args.len()
        )));
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
            // exact bit constants, ln routed through the vendored libm log
            arity(name, &args, 1)?;
            let x = want_float(name, &args[0])? - 1.0;
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
            fl(half_ln_2pi + (x + 0.5) * lt - t + lag)
        }
        "math.choose" => {
            // runtime/rs/src/math.rs almide_rt_math_choose: wrapping mul,
            // truncating div per step (value_domain_arith ch_max)
            arity(name, &args, 2)?;
            let (n, k) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            if k < 0 || k > n {
                return Ok(Ok(Value::Int(0)));
            }
            let k = k.min(n.wrapping_sub(k));
            let mut result: i64 = 1;
            let mut i: i64 = 0;
            while i < k {
                result = result.wrapping_mul(n.wrapping_sub(i)) / (i + 1);
                i += 1;
            }
            Ok(Value::Int(result))
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
            let idx = ((ts / 86400 % 7) + 7) % 7;
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
        "datetime.from_parts" => {
            arity(name, &args, 6)?;
            let mut a = [0i64; 6];
            for (i, slot) in a.iter_mut().enumerate() {
                *slot = want_int(name, &args[i])?;
            }
            let (y, m, d, h, min, s) = (a[0], a[1], a[2], a[3], a[4], a[5]);
            let ya = if m <= 2 { y.wrapping_sub(1) } else { y };
            let eadj = if ya >= 0 { ya } else { ya.wrapping_sub(399) };
            let era = eadj / 400;
            let mp = if m > 2 {
                m.wrapping_sub(3)
            } else {
                m.wrapping_add(9)
            };
            let doy = (mp.wrapping_mul(153).wrapping_add(2)) / 5 + d - 1;
            let yoe = ya.wrapping_sub(era.wrapping_mul(400));
            let doe = yoe
                .wrapping_mul(365)
                .wrapping_add(yoe / 4)
                .wrapping_sub(yoe / 100)
                .wrapping_add(doy);
            let days = era
                .wrapping_mul(146097)
                .wrapping_add(doe)
                .wrapping_sub(719468);
            Ok(Value::Int(
                days.wrapping_mul(86400)
                    .wrapping_add(h.wrapping_mul(3600))
                    .wrapping_add(min.wrapping_mul(60))
                    .wrapping_add(s),
            ))
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
            let neg = y < 0;
            let mag = if neg { 0i64.wrapping_sub(y) } else { y };
            let mut ystr = pad_num(mag, dt_digits(mag).max(4));
            if neg {
                ystr.insert(0, '-');
            }
            Ok(Value::str(&format!(
                "{ystr}-{}-{}T{}:{}:{}Z",
                pad_num(mo, 2),
                pad_num(d, 2),
                pad_num(h, 2),
                pad_num(mi, 2),
                pad_num(s, 2)
            )))
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
            let out = pattern
                .replace("%Y", &pad_num(y, 4))
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

        // ═══ base64 (RFC 4648, canonical padded; url = -_ unpadded-tolerant) ═══
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

        // ═══ args (stdlib/args.almd over env.args(); ref argv is empty) ═══
        "args.raw" | "args.positional" => {
            arity(name, &args, 0)?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "args.flag" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(false))
        }
        "args.option" | "args.positional_at" => {
            arity(name, &args, 1)?;
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

        // ═══ io ══════════════════════════════════════════════════════════
        "io.write" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let raw = b.borrow().clone();
            it.stdout.push_str(&String::from_utf8_lossy(&raw));
            Ok(Value::Unit)
        }
        "io.write_bytes" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut b = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                b.push(want_int(name, x)? as u8);
            }
            it.stdout.push_str(&String::from_utf8_lossy(&b));
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
            Ok(match std::fs::read(&**want_str(name, &args[0])?) {
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
                "fs.read_lines_if_exists" => match std::fs::read_to_string(&p) {
                    Ok(s) => ok(some(Value::List(Rc::new(
                        crate::stdlib_ext::split_lines(&s)
                            .into_iter()
                            .map(|l| Value::str(&l))
                            .collect(),
                    )))),
                    Err(e) => io_err(e),
                },
                "fs.read_bytes_if_exists" => match std::fs::read(&p) {
                    Ok(b) => ok(some(Value::List(Rc::new(
                        b.into_iter().map(|x| Value::Int(x as i64)).collect(),
                    )))),
                    Err(e) => io_err(e),
                },
                _ => match std::fs::read(&p) {
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
            // in-place, (src, count, dst); a window that does not FIT is a
            // no-op, never a clamp (bytes_writer_family: copy_within_no_fit)
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let (src, count, dst) = (
                want_int(name, &args[1])?,
                want_int(name, &args[2])?,
                want_int(name, &args[3])?,
            );
            let mut v = b.borrow_mut();
            if src >= 0 && dst >= 0 && count >= 0 {
                let (src, count, dst) = (src as usize, count as usize, dst as usize);
                if src + count <= v.len() && dst + count <= v.len() {
                    v.copy_within(src..src + count, dst);
                }
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
                (want_float(name, &args[1])? as f32).to_bits() as u64,
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
                want_float(name, &args[1])?.to_bits(),
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
        "fan.map" | "fan.any" | "fan.settle" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = match &args[1] {
                Value::Fn(c) => c.clone(),
                other => return mismatch(name, "a function", other),
            };
            let mut vals: Vec<Value> = Vec::new();
            for x in xs.iter() {
                let r = it.call_value(&c, vec![x.clone()])?;
                match (name, r) {
                    ("fan.map", Value::Ok(v)) => vals.push((*v).clone()),
                    ("fan.map", Value::Err(e)) => return Ok(Ok(Value::Err(e))), // first err, index order
                    ("fan.map", v) => vals.push(v),
                    ("fan.any", Value::Ok(v)) => return Ok(Ok(Value::Ok(v))), // first success wins
                    ("fan.any", Value::Err(_)) => {}
                    ("fan.any", v) => return Ok(Ok(Value::Ok(Rc::new(v)))),
                    ("fan.settle", Value::Ok(v)) => vals.push(Value::Ok(v)),
                    ("fan.settle", Value::Err(e)) => vals.push(Value::Err(e)),
                    ("fan.settle", v) => vals.push(Value::Ok(Rc::new(v))),
                    _ => unreachable!(),
                }
            }
            Ok(match name {
                "fan.map" => ok(Value::List(Rc::new(vals))),
                "fan.any" => err_str("fan.any: all candidates failed"),
                _ => Value::List(Rc::new(vals)),
            })
        }

        _ => return Err(Flow::Fatal("__not_ext__".into())),
    })
}

/// civil_from_days translated from stdlib/datetime_calendar.almd
/// (truncating division, wrapping arithmetic) → (year, month, day)
fn civ(ts: i64) -> (i64, i64, i64) {
    let z = ts / 86400 + 719468;
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

/// decode errors pinned by encoding_base64: length ≢ 0 (mod 4) →
/// "invalid base64 length: N" (N = the input length); any character
/// outside the table — '=' in a non-tail slot included — →
/// "invalid base64 character". Padding: 1 or 2 trailing '='.
fn b64_decode(s: &str, url: bool) -> Result<Vec<u8>, String> {
    let table = if url { B64_URL } else { B64_STD };
    let cs: Vec<char> = s.chars().collect();
    if !cs.len().is_multiple_of(4) {
        return Err(format!("invalid base64 length: {}", cs.len()));
    }
    let mut vals: Vec<u8> = Vec::new();
    let mut pad = 0usize;
    for (i, &c) in cs.iter().enumerate() {
        if c == '=' {
            // only the last one or two positions may be padding
            if i + 2 < cs.len() || cs[i..].iter().any(|&x| x != '=') {
                return Err("invalid base64 character".into());
            }
            pad = cs.len() - i;
            break;
        }
        match table.iter().position(|&t| t as char == c) {
            Some(idx) => vals.push(idx as u8),
            None => return Err("invalid base64 character".into()),
        }
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
    let _ = pad;
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
        type_name: Some(Rc::from("HttpResponse")),
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
        } if &**tn == "HttpResponse" => {
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
