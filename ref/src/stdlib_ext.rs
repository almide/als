//! Round-2 stdlib modules: the dynamic `Value` (ALS-D2/D6), `json` (ALS-T3,
//! D1, D3), `bytes` (ALS-D5/D7), and the effectful `fs` / `env` / `io` /
//! `process` surfaces. Same doctrine as stdlib.rs: written from the chapters
//! and the fixtures those chapters cite; unknown edges abstain with a class.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::value::{dyn_text, json_quote, values_eq, Dyn, PathSeg, Value, F64};

pub const EXT_FNS: &[&str] = &[
    // value
    "value.field",
    "value.keys",
    "value.as_string",
    "value.as_int",
    "value.as_float",
    "value.as_bool",
    "value.as_array",
    "value.str",
    "value.int",
    "value.float",
    "value.bool",
    "value.object",
    "value.array",
    "value.null",
    "value.pick",
    "value.omit",
    "value.merge",
    "value.stringify",
    // json
    "json.parse",
    "json.stringify",
    "json.stringify_pretty",
    "json.get_string",
    "json.get_int",
    "json.get_float",
    "json.get_bool",
    "json.get_array",
    "json.root",
    "json.field",
    "json.index",
    "json.get_path",
    "json.set_path",
    "json.remove_path",
    "json.null",
    "json.object",
    "json.array",
    "json.keys",
    "json.from_string",
    "json.from_int",
    "json.from_bool",
    "json.from_float",
    "json.get",
    // bytes
    "bytes.new",
    "bytes.from_list",
    "bytes.from_string",
    "bytes.to_list",
    "bytes.len",
    "bytes.is_empty",
    "bytes.get",
    "bytes.get_or",
    "bytes.slice",
    "bytes.concat",
    "bytes.repeat",
    "bytes.set",
    "bytes.push",
    "bytes.clear",
    "bytes.append",
    "bytes.to_string",
    "bytes.set_at",
    "bytes.fill",
    "bytes.copy_from",
    "bytes.read_string_at",
    "bytes.read_length_prefixed_strings_le",
    "bytes.skip_length_prefixed_le",
    "bytes.read_u8",
    "bytes.read_u16_le",
    "bytes.read_u32_le",
    "bytes.read_i32_le",
    "bytes.read_i64_le",
    "bytes.read_f16_le",
    "bytes.read_f32_le",
    "bytes.read_f64_le",
    "bytes.read_u16_be",
    "bytes.read_u32_be",
    "bytes.read_i32_be",
    "bytes.read_i64_be",
    "bytes.read_f32_be",
    "bytes.read_f64_be",
    "bytes.set_u8",
    "bytes.set_u16_le",
    "bytes.set_u32_le",
    "bytes.set_i32_le",
    "bytes.set_i64_le",
    "bytes.set_f32_le",
    "bytes.set_f64_le",
    "bytes.set_u16_be",
    "bytes.set_u32_be",
    "bytes.set_i32_be",
    "bytes.set_i64_be",
    "bytes.set_f32_be",
    "bytes.set_f64_be",
    "bytes.append_u8",
    "bytes.append_u16_le",
    "bytes.append_u32_le",
    "bytes.append_i32_le",
    "bytes.append_i64_le",
    "bytes.append_f32_le",
    "bytes.append_f64_le",
    "bytes.append_u16_be",
    "bytes.append_u32_be",
    "bytes.append_i32_be",
    "bytes.append_i64_be",
    "bytes.append_f32_be",
    "bytes.append_f64_be",
    // fs / env / io / process
    "fs.read_text",
    "fs.read_bytes",
    "fs.write",
    "fs.write_bytes",
    "fs.append",
    "fs.mkdir_p",
    "fs.exists",
    "fs.read_lines",
    "fs.remove",
    "fs.list_dir",
    "fs.is_dir",
    "fs.is_file",
    "fs.copy",
    "fs.rename",
    "fs.remove_all",
    "fs.file_size",
    "fs.temp_dir",
    "fs.create_temp_file",
    "fs.create_temp_dir",
    "fs.read_text_if_exists",
    "fs.fold_lines",
    "fs.for_each_line",
    "env.args",
    "env.get",
    "env.set",
    "env.cwd",
    "env.temp_dir",
    "env.os",
    "io.print",
    "process.args",
    "process.exit",
    // late arrivals for the core modules
    "string.push",
    "list.group_by",
];

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

fn want_bool(name: &str, v: &Value) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => mismatch(name, "Bool", other),
    }
}

fn want_list<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<Value>>, Flow> {
    match v {
        Value::List(xs) => Ok(xs),
        other => mismatch(name, "List", other),
    }
}

fn want_dyn<'a>(name: &str, v: &'a Value) -> Result<&'a Dyn, Flow> {
    match v {
        Value::Dyn(d) => Ok(d),
        other => mismatch(name, "Value", other),
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

fn want_path<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<PathSeg>>, Flow> {
    match v {
        Value::Path(p) => Ok(p),
        other => mismatch(name, "JsonPath", other),
    }
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

fn io_err(e: std::io::Error) -> Value {
    err_str(&e.to_string())
}

/// dispatch; None = not an ext function (caller abstains)
pub fn call_ext(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    match dispatch(it, name, args) {
        Ok(r) => Some(r),
        Err(Flow::Fatal(m)) if &*m == "__not_ext__" => None,
        Err(f) => Some(Err(f)),
    }
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Result<Value, Flow>, Flow> {
    // the inner Result is the actual outcome; the outer lets `?` flow argument errors
    Ok(match name {
        // ═══ value ═════════════════════════════════════════════════════
        "value.null" | "json.null" => {
            arity(name, &args, 0)?;
            Ok(Value::Dyn(Dyn::Null))
        }
        "value.int" | "json.from_int" => {
            arity(name, &args, 1)?;
            Ok(Value::Dyn(Dyn::I(want_int(name, &args[0])?)))
        }
        "value.float" | "json.from_float" => {
            arity(name, &args, 1)?;
            Ok(Value::Dyn(Dyn::F(want_float(name, &args[0])?)))
        }
        "value.bool" | "json.from_bool" => {
            arity(name, &args, 1)?;
            Ok(Value::Dyn(Dyn::B(want_bool(name, &args[0])?)))
        }
        "value.str" | "json.from_string" => {
            arity(name, &args, 1)?;
            Ok(Value::Dyn(Dyn::S(want_str(name, &args[0])?.clone())))
        }
        "value.array" | "json.array" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut out = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                out.push(want_dyn(name, x)?.clone());
            }
            Ok(Value::Dyn(Dyn::A(Rc::new(out))))
        }
        "value.object" | "json.object" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut out: Vec<(Rc<str>, Dyn)> = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                let (k, v) = match x {
                    Value::Tuple(p) if p.len() == 2 => (
                        want_str(name, &p[0])?.clone(),
                        want_dyn(name, &p[1])?.clone(),
                    ),
                    other => return mismatch(name, "List[(String, Value)]", other),
                };
                match out.iter_mut().find(|(k2, _)| *k2 == k) {
                    Some(slot) => slot.1 = v,
                    None => out.push((k, v)),
                }
            }
            Ok(Value::Dyn(Dyn::O(Rc::new(out))))
        }
        "value.keys" | "json.keys" => {
            arity(name, &args, 1)?;
            match want_dyn(name, &args[0])? {
                Dyn::O(fields) => Ok(Value::List(Rc::new(
                    fields.iter().map(|(k, _)| Value::Str(k.clone())).collect(),
                ))),
                _ => Ok(Value::List(Rc::new(Vec::new()))),
            }
        }
        "value.field" => {
            arity(name, &args, 2)?;
            let key = want_str(name, &args[1])?.clone();
            match want_dyn(name, &args[0])? {
                Dyn::O(fields) => Ok(match fields.iter().find(|(k, _)| *k == key) {
                    Some((_, v)) => ok(Value::Dyn(v.clone())),
                    None => err_str(&format!("field '{key}' not found")),
                }),
                _ => Ok(err_str("expected Object")),
            }
        }
        "json.get" => {
            arity(name, &args, 2)?;
            let key = want_str(name, &args[1])?.clone();
            match want_dyn(name, &args[0])? {
                Dyn::O(fields) => Ok(fields
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| some(Value::Dyn(v.clone())))
                    .unwrap_or(Value::None)),
                _ => Ok(Value::None),
            }
        }
        "value.as_string" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::S(s) => ok(Value::Str(s.clone())),
                _ => err_str("expected Str"),
            })
        }
        "value.as_int" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::I(n) => ok(Value::Int(*n)),
                _ => err_str("expected Int"),
            })
        }
        "value.as_float" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::F(f) => ok(Value::Float(F64(*f))),
                Dyn::I(n) => ok(Value::Float(F64(*n as f64))), // D6: integer-formed numbers widen
                _ => err_str("expected Float"),
            })
        }
        "value.as_bool" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::B(b) => ok(Value::Bool(*b)),
                _ => err_str("expected Bool"),
            })
        }
        "value.as_array" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::A(items) => ok(Value::List(Rc::new(
                    items.iter().map(|d| Value::Dyn(d.clone())).collect(),
                ))),
                _ => err_str("expected Array"),
            })
        }
        "value.pick" | "value.omit" => {
            arity(name, &args, 2)?;
            let keys = want_list(name, &args[1])?.clone();
            let mut names: Vec<Rc<str>> = Vec::new();
            for k in keys.iter() {
                names.push(want_str(name, k)?.clone());
            }
            match want_dyn(name, &args[0])? {
                Dyn::O(fields) => {
                    let keep = |k: &Rc<str>| {
                        let hit = names.iter().any(|n| n == k);
                        if name == "value.pick" {
                            hit
                        } else {
                            !hit
                        }
                    };
                    Ok(Value::Dyn(Dyn::O(Rc::new(
                        fields.iter().filter(|(k, _)| keep(k)).cloned().collect(),
                    ))))
                }
                other => Ok(Value::Dyn(other.clone())),
            }
        }
        "value.merge" => {
            arity(name, &args, 2)?;
            match (want_dyn(name, &args[0])?, want_dyn(name, &args[1])?) {
                (Dyn::O(a), Dyn::O(b)) => {
                    let mut out: Vec<(Rc<str>, Dyn)> = (**a).clone();
                    for (k, v) in b.iter() {
                        match out.iter_mut().find(|(k2, _)| k2 == k) {
                            Some(slot) => slot.1 = v.clone(),
                            None => out.push((k.clone(), v.clone())),
                        }
                    }
                    Ok(Value::Dyn(Dyn::O(Rc::new(out))))
                }
                (_, b) => Ok(Value::Dyn(b.clone())),
            }
        }
        "value.stringify" | "json.stringify" => {
            arity(name, &args, 1)?;
            Ok(Value::str(&dyn_text(want_dyn(name, &args[0])?)))
        }
        "json.stringify_pretty" => {
            arity(name, &args, 1)?;
            Ok(Value::str(&pretty(want_dyn(name, &args[0])?, 0)))
        }
        "json.parse" => {
            arity(name, &args, 1)?;
            Ok(json_parse(want_str(name, &args[0])?))
        }
        "json.get_string" | "json.get_int" | "json.get_float" | "json.get_bool"
        | "json.get_array" => {
            arity(name, &args, 2)?;
            let key = want_str(name, &args[1])?.clone();
            let field = match want_dyn(name, &args[0])? {
                Dyn::O(fields) => fields
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone()),
                _ => None,
            };
            Ok(match (name, field) {
                (_, None) => Value::None,
                ("json.get_string", Some(Dyn::S(s))) => some(Value::Str(s)),
                ("json.get_int", Some(Dyn::I(n))) => some(Value::Int(n)),
                ("json.get_float", Some(Dyn::F(f))) => some(Value::Float(F64(f))),
                ("json.get_float", Some(Dyn::I(n))) => some(Value::Float(F64(n as f64))),
                ("json.get_bool", Some(Dyn::B(b))) => some(Value::Bool(b)),
                ("json.get_array", Some(Dyn::A(items))) => some(Value::List(Rc::new(
                    items.iter().map(|d| Value::Dyn(d.clone())).collect(),
                ))),
                _ => Value::None,
            })
        }
        "json.root" => {
            arity(name, &args, 0)?;
            Ok(Value::Path(Rc::new(Vec::new())))
        }
        "json.field" => {
            arity(name, &args, 2)?;
            let mut p = (**want_path(name, &args[0])?).clone();
            p.push(PathSeg::Field(want_str(name, &args[1])?.clone()));
            Ok(Value::Path(Rc::new(p)))
        }
        "json.index" => {
            arity(name, &args, 2)?;
            let mut p = (**want_path(name, &args[0])?).clone();
            p.push(PathSeg::Index(want_int(name, &args[1])?));
            Ok(Value::Path(Rc::new(p)))
        }
        "json.get_path" => {
            arity(name, &args, 2)?;
            let mut cur = want_dyn(name, &args[0])?.clone();
            for seg in want_path(name, &args[1])?.iter() {
                match (seg, &cur) {
                    (PathSeg::Field(k), Dyn::O(fields)) => {
                        match fields.iter().find(|(k2, _)| k2 == k) {
                            Some((_, v)) => cur = v.clone(),
                            None => return Ok(Ok(Value::None)),
                        }
                    }
                    (PathSeg::Index(i), Dyn::A(items)) => {
                        if *i < 0 || *i as usize >= items.len() {
                            return Ok(Ok(Value::None));
                        }
                        cur = items[*i as usize].clone();
                    }
                    _ => return Ok(Ok(Value::None)), // D1: type-mismatch node degrades to none
                }
            }
            Ok(some(Value::Dyn(cur)))
        }
        "json.set_path" | "json.remove_path" => {
            // D1 pins the edges against the serde_json oracle; the write forms
            // are rarer — take them in a later round rather than guess
            return Err(Flow::Abstain {
                class: format!("stdlib:{name}"),
                reason: "json path writes are not implemented by the reference evaluator yet"
                    .into(),
            });
        }
        "json.to_map" => {
            arity(name, &args, 1)?;
            Ok(match want_dyn(name, &args[0])? {
                Dyn::O(fields) => {
                    let mut out: Vec<(Value, Value)> = Vec::new();
                    for (k, v) in fields.iter() {
                        match v {
                            Dyn::S(s) => out.push((Value::Str(k.clone()), Value::Str(s.clone()))),
                            _ => return Ok(Ok(Value::None)),
                        }
                    }
                    some(Value::Map(Rc::new(out)))
                }
                _ => Value::None,
            })
        }

        // ═══ bytes (ALS-D5/D7) — REFERENCE semantics, total reads ═══════
        "bytes.new" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?.max(0) as usize;
            // C-197: an unsatisfiable allocation is the DEFINED abort
            let mut v: Vec<u8> = Vec::new();
            if v.try_reserve_exact(n).is_err() {
                return Ok(Err(Flow::Abort("out of memory".into())));
            }
            v.resize(n, 0);
            Ok(bytes_val(v))
        }
        "bytes.from_list" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut out = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                out.push(want_int(name, x)? as u8); // D7: values carried as-is (low byte)
            }
            Ok(bytes_val(out))
        }
        "bytes.from_string" => {
            arity(name, &args, 1)?;
            Ok(bytes_val(want_str(name, &args[0])?.as_bytes().to_vec()))
        }
        "bytes.to_string" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let s = String::from_utf8_lossy(&b.borrow()).to_string();
            Ok(Value::str(&s))
        }
        "bytes.to_list" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let out: Vec<Value> = b.borrow().iter().map(|x| Value::Int(*x as i64)).collect();
            Ok(Value::List(Rc::new(out)))
        }
        "bytes.len" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let n = b.borrow().len() as i64;
            Ok(Value::Int(n))
        }
        "bytes.is_empty" => {
            arity(name, &args, 1)?;
            let b = want_bytes(name, &args[0])?;
            let e = b.borrow().is_empty();
            Ok(Value::Bool(e))
        }
        "bytes.get" | "bytes.get_or" => {
            arity(name, &args, if name == "bytes.get" { 2 } else { 3 })?;
            let b = want_bytes(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            let hit = if i < 0 {
                None
            } else {
                b.borrow().get(i as usize).copied()
            };
            Ok(match (name, hit) {
                ("bytes.get", Some(v)) => some(Value::Int(v as i64)),
                ("bytes.get", None) => Value::None,
                (_, Some(v)) => Value::Int(v as i64),
                (_, None) => args[2].clone(),
            })
        }
        "bytes.slice" => {
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let a = (want_int(name, &args[1])? as u64).min(b.len() as u64) as usize;
            let e = (want_int(name, &args[2])? as u64).min(b.len() as u64) as usize;
            Ok(bytes_val(if a >= e {
                Vec::new()
            } else {
                b[a..e].to_vec()
            }))
        }
        "bytes.concat" => {
            arity(name, &args, 2)?;
            let x = want_bytes(name, &args[0])?;
            let y = want_bytes(name, &args[1])?;
            let mut v = x.borrow().clone();
            v.extend_from_slice(&y.borrow());
            Ok(bytes_val(v))
        }
        "bytes.repeat" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let n0 = want_int(name, &args[1])?;
            if n0 > 0 && (b.len() as i128) * (n0 as i128) > (1i128 << 31) {
                return Ok(Err(Flow::Abort("repeat result too large".into())));
            }
            let n = n0.max(0) as usize;
            let mut v = Vec::with_capacity(b.len() * n);
            for _ in 0..n {
                v.extend_from_slice(&b);
            }
            Ok(bytes_val(v))
        }
        "bytes.set" => {
            // the FUNCTIONAL single-byte replace: returns a new buffer
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            let mut v = b.borrow().clone();
            if i >= 0 && (i as usize) < v.len() {
                v[i as usize] = want_int(name, &args[2])? as u8;
            }
            Ok(bytes_val(v))
        }
        "bytes.push" | "bytes.append_u8" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let x = want_int(name, &args[1])? as u8;
            b.borrow_mut().push(x);
            Ok(Value::Unit)
        }
        "bytes.append" => {
            arity(name, &args, 2)?;
            let dst = want_bytes(name, &args[0])?;
            let src = want_bytes(name, &args[1])?;
            let add = src.borrow().clone(); // aliasing-safe
            dst.borrow_mut().extend_from_slice(&add);
            Ok(Value::Unit)
        }
        "bytes.clear" => {
            arity(name, &args, 1)?;
            want_bytes(name, &args[0])?.borrow_mut().clear();
            Ok(Value::Unit)
        }
        "bytes.set_at" => {
            // in-place single byte; OOB is a silent no-op (bytes_writer_family)
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            let x = want_int(name, &args[2])? as u8;
            let mut v = b.borrow_mut();
            if i >= 0 && (i as usize) < v.len() {
                v[i as usize] = x;
            }
            Ok(Value::Unit)
        }
        "bytes.fill" => {
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let x = want_int(name, &args[1])? as u8;
            for slot in b.borrow_mut().iter_mut() {
                *slot = x;
            }
            Ok(Value::Unit)
        }
        "bytes.copy_from" => {
            // clamps to what fits (bytes_writer_family copy_from_clamped)
            arity(name, &args, 5)?;
            let dst = want_bytes(name, &args[0])?;
            let src = want_bytes(name, &args[1])?;
            let (d, s0, n) = (
                want_int(name, &args[2])?,
                want_int(name, &args[3])?,
                want_int(name, &args[4])?,
            );
            if d >= 0 && s0 >= 0 && n > 0 {
                let srcv = src.borrow().clone(); // aliasing-safe
                let mut v = dst.borrow_mut();
                let (d, s0, n) = (d as usize, s0 as usize, n as usize);
                let n = n
                    .min(srcv.len().saturating_sub(s0))
                    .min(v.len().saturating_sub(d));
                v[d..d + n].copy_from_slice(&srcv[s0..s0 + n]);
            }
            Ok(Value::Unit)
        }
        "bytes.read_string_be" => {
            // u32 BE length prefix + UTF-8 bytes (the read twin of
            // write_string_be); any OOB window yields ""
            arity(name, &args, 2)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let p = want_int(name, &args[1])?;
            if p < 0 || (p as u64).saturating_add(4) > b.len() as u64 {
                return Ok(Ok(Value::str("")));
            }
            let q = p as usize;
            let len = (u32::from(b[q]) << 24
                | u32::from(b[q + 1]) << 16
                | u32::from(b[q + 2]) << 8
                | u32::from(b[q + 3])) as u64;
            if (q as u64 + 4).saturating_add(len) > b.len() as u64 {
                return Ok(Ok(Value::str("")));
            }
            let s = String::from_utf8_lossy(&b[q + 4..q + 4 + len as usize]).to_string();
            Ok(Value::str(&s))
        }
        "bytes.read_string_at" => {
            // any OOB/negative window yields "" (bytes_string_window_domain)
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let (p, n) = (want_int(name, &args[1])?, want_int(name, &args[2])?);
            if p < 0 || n < 0 || (p as u64).saturating_add(n as u64) > b.len() as u64 {
                return Ok(Ok(Value::str("")));
            }
            let s = String::from_utf8_lossy(&b[p as usize..(p + n) as usize]).to_string();
            Ok(Value::str(&s))
        }
        "bytes.read_length_prefixed_strings_le" | "bytes.skip_length_prefixed_le" => {
            // totality per bytes_string_window_domain: an unreadable length
            // prefix STOPS the walk (skip returns the position reached;
            // the reader returns what it collected)
            arity(name, &args, 3)?;
            let b = want_bytes(name, &args[0])?;
            let b = b.borrow();
            let (p0, count) = (want_int(name, &args[1])?, want_int(name, &args[2])?);
            let mut p: i64 = p0;
            let mut out: Vec<Value> = Vec::new();
            let mut k: i64 = 0;
            while k < count {
                if p < 0 || (p as u64).saturating_add(4) > b.len() as u64 {
                    break;
                }
                let q = p as usize;
                let len = u32::from(b[q])
                    | u32::from(b[q + 1]) << 8
                    | u32::from(b[q + 2]) << 16
                    | u32::from(b[q + 3]) << 24;
                let body = q + 4;
                if (body as u64).saturating_add(len as u64) > b.len() as u64 {
                    break;
                }
                if name == "bytes.read_length_prefixed_strings_le" {
                    let s = String::from_utf8_lossy(&b[body..body + len as usize]).to_string();
                    out.push(Value::str(&s));
                }
                p = (body + len as usize) as i64;
                k += 1;
            }
            Ok(if name == "bytes.skip_length_prefixed_le" {
                Value::Int(p)
            } else {
                Value::List(Rc::new(out))
            })
        }
        n2 if n2.starts_with("bytes.read_") => read_bytes_fn(it, n2, &args)?,
        n2 if n2.starts_with("bytes.set_") || n2.starts_with("bytes.append_") => {
            write_bytes_fn(it, n2, &args)?
        }

        // ═══ fs / env / io / process ═══════════════════════════════════
        "fs.read_text" => {
            arity(name, &args, 1)?;
            Ok(
                match std::fs::read_to_string(&**want_str(name, &args[0])?) {
                    Ok(s) => ok(Value::str(&s)),
                    Err(e) => io_err(e),
                },
            )
        }
        "fs.read_text_if_exists" => {
            arity(name, &args, 1)?;
            let p = want_str(name, &args[0])?;
            Ok(if std::path::Path::new(&**p).exists() {
                match std::fs::read_to_string(&**p) {
                    Ok(s) => ok(some(Value::str(&s))),
                    Err(e) => io_err(e),
                }
            } else {
                ok(Value::None)
            })
        }
        "fs.read_bytes" => {
            arity(name, &args, 1)?;
            Ok(match std::fs::read(&**want_str(name, &args[0])?) {
                Ok(b) => ok(Value::List(Rc::new(
                    b.into_iter().map(|x| Value::Int(x as i64)).collect(),
                ))),
                Err(e) => io_err(e),
            })
        }
        "fs.write" | "fs.append" => {
            arity(name, &args, 2)?;
            let p = want_str(name, &args[0])?.to_string();
            let c = want_str(name, &args[1])?.to_string();
            let r = if name == "fs.write" {
                std::fs::write(&p, c.as_bytes())
            } else {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&p)
                    .and_then(|mut f| f.write_all(c.as_bytes()))
            };
            Ok(match r {
                Ok(()) => ok(Value::Unit),
                Err(e) => io_err(e),
            })
        }
        "fs.write_bytes" => {
            arity(name, &args, 2)?;
            let p = want_str(name, &args[0])?.to_string();
            let xs = want_list(name, &args[1])?.clone();
            let mut b = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                b.push(want_int(name, x)? as u8);
            }
            Ok(match std::fs::write(&p, b) {
                Ok(()) => ok(Value::Unit),
                Err(e) => io_err(e),
            })
        }
        "fs.mkdir_p" => {
            arity(name, &args, 1)?;
            Ok(
                match std::fs::create_dir_all(&**want_str(name, &args[0])?) {
                    Ok(()) => ok(Value::Unit),
                    Err(e) => io_err(e),
                },
            )
        }
        "fs.exists" | "fs.is_dir" | "fs.is_file" => {
            arity(name, &args, 1)?;
            let p = std::path::Path::new(&**want_str(name, &args[0])?).to_path_buf();
            Ok(Value::Bool(match name {
                "fs.exists" => p.exists(),
                "fs.is_dir" => p.is_dir(),
                _ => p.is_file(),
            }))
        }
        "fs.read_lines" => {
            arity(name, &args, 1)?;
            Ok(
                match std::fs::read_to_string(&**want_str(name, &args[0])?) {
                    Ok(s) => ok(Value::List(Rc::new(
                        split_lines(&s)
                            .into_iter()
                            .map(|l| Value::str(&l))
                            .collect(),
                    ))),
                    Err(e) => io_err(e),
                },
            )
        }
        "fs.fold_lines" | "fs.for_each_line" => {
            // ALS-R7: fallible callbacks first-err at the line; observable is
            // the callback call sequence
            let cb_at = if name == "fs.fold_lines" { 2 } else { 1 };
            arity(name, &args, cb_at + 1)?;
            let text = match std::fs::read_to_string(&**want_str(name, &args[0])?) {
                Ok(s) => s,
                Err(e) => return Ok(Ok(io_err(e))),
            };
            let c = match &args[cb_at] {
                Value::Fn(c) => c.clone(),
                other => return mismatch(name, "a function", other),
            };
            let fal = it.cb_fallible(&c);
            let mut acc = if name == "fs.fold_lines" {
                args[1].clone()
            } else {
                Value::Unit
            };
            for line in split_lines(&text) {
                let cb_args = if name == "fs.fold_lines" {
                    vec![acc.clone(), Value::str(&line)]
                } else {
                    vec![Value::str(&line)]
                };
                let v = it.call_value(&c, cb_args)?;
                let v = if fal {
                    match v {
                        Value::Ok(x) => (*x).clone(),
                        Value::Err(e) => return Ok(Ok(Value::Err(e))),
                        other => other,
                    }
                } else {
                    v
                };
                if name == "fs.fold_lines" {
                    acc = v;
                }
            }
            Ok(ok(if name == "fs.fold_lines" {
                acc
            } else {
                Value::Unit
            }))
        }
        "fs.remove" | "fs.remove_all" => {
            arity(name, &args, 1)?;
            let p = std::path::Path::new(&**want_str(name, &args[0])?).to_path_buf();
            let r = if name == "fs.remove_all" {
                if p.is_dir() {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                }
            } else {
                std::fs::remove_file(&p)
            };
            Ok(match r {
                Ok(()) => ok(Value::Unit),
                Err(e) => io_err(e),
            })
        }
        "fs.list_dir" => {
            arity(name, &args, 1)?;
            // ALS-R6: all entries, `.`/`..` excluded, BYTE-lexicographic order
            Ok(match std::fs::read_dir(&**want_str(name, &args[0])?) {
                Ok(rd) => {
                    let mut names: Vec<String> = Vec::new();
                    for e in rd.flatten() {
                        names.push(e.file_name().to_string_lossy().to_string());
                    }
                    // insertion sort by byte order (host sort is forbidden)
                    let mut sorted: Vec<String> = Vec::with_capacity(names.len());
                    for n in names {
                        let pos = sorted
                            .iter()
                            .position(|m| m.as_bytes() > n.as_bytes())
                            .unwrap_or(sorted.len());
                        sorted.insert(pos, n);
                    }
                    ok(Value::List(Rc::new(
                        sorted.into_iter().map(|n| Value::str(&n)).collect(),
                    )))
                }
                Err(e) => io_err(e),
            })
        }
        "fs.copy" | "fs.rename" => {
            arity(name, &args, 2)?;
            let a = want_str(name, &args[0])?.to_string();
            let b = want_str(name, &args[1])?.to_string();
            let r = if name == "fs.copy" {
                std::fs::copy(&a, &b).map(|_| ())
            } else {
                std::fs::rename(&a, &b)
            };
            Ok(match r {
                Ok(()) => ok(Value::Unit),
                Err(e) => io_err(e),
            })
        }
        "fs.file_size" => {
            arity(name, &args, 1)?;
            Ok(match std::fs::metadata(&**want_str(name, &args[0])?) {
                Ok(m) => ok(Value::Int(m.len() as i64)),
                Err(e) => io_err(e),
            })
        }
        "fs.temp_dir" | "env.temp_dir" => {
            arity(name, &args, 0)?;
            Ok(Value::str(&std::env::temp_dir().to_string_lossy()))
        }
        "fs.create_temp_file" | "fs.create_temp_dir" => {
            arity(name, &args, 1)?;
            let prefix = want_str(name, &args[0])?.to_string();
            let base = std::env::temp_dir();
            let mut i: u32 = 0;
            Ok(loop {
                let cand = base.join(format!("{prefix}{}-{i}", std::process::id()));
                let r = if name == "fs.create_temp_dir" {
                    std::fs::create_dir(&cand).map(|_| cand.clone())
                } else {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&cand)
                        .map(|_| cand.clone())
                };
                match r {
                    Ok(p) => break ok(Value::str(&p.to_string_lossy())),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && i < 10_000 => i += 1,
                    Err(e) => break io_err(e),
                }
            })
        }
        "env.args" => {
            arity(name, &args, 0)?;
            // ALS-R5: argv without the program name; the ref protocol runs
            // the program with no arguments
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "process.args" => {
            arity(name, &args, 0)?;
            // process.args INCLUDES argv0 (process_args: len 1, nonempty);
            // its content is host-specific and never pinned beyond that
            Ok(Value::List(Rc::new(vec![Value::str("als-ref")])))
        }
        "env.get" => {
            arity(name, &args, 1)?;
            let k = want_str(name, &args[0])?.to_string();
            Ok(match it.env_overlay.iter().rev().find(|(n, _)| *n == k) {
                Some((_, v)) => some(Value::str(v)),
                None => match std::env::var(&k) {
                    Ok(v) => some(Value::str(&v)),
                    Err(_) => Value::None,
                },
            })
        }
        "env.set" => {
            arity(name, &args, 2)?;
            let k = want_str(name, &args[0])?.to_string();
            let v = want_str(name, &args[1])?.to_string();
            it.env_overlay.push((k, v));
            Ok(Value::Unit)
        }
        "env.cwd" => {
            arity(name, &args, 0)?;
            Ok(match std::env::current_dir() {
                Ok(p) => ok(Value::str(&p.to_string_lossy())),
                Err(e) => io_err(e),
            })
        }
        "env.os" => {
            arity(name, &args, 0)?;
            // ALS-R5: the one sanctioned host observation — the closed set
            Ok(Value::str(if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(target_os = "windows") {
                "windows"
            } else {
                "linux"
            }))
        }
        "io.print" => {
            arity(name, &args, 1)?;
            let s = want_str(name, &args[0])?.to_string();
            it.stdout.push_str(&s);
            Ok(Value::Unit)
        }
        "process.exit" => {
            arity(name, &args, 1)?;
            return Err(Flow::Exit(want_int(name, &args[0])? as i32));
        }

        // ═══ late arrivals ═════════════════════════════════════════════
        "string.push" => {
            arity(name, &args, 2)?;
            let mut s = want_str(name, &args[0])?.to_string();
            s.push_str(want_str(name, &args[1])?);
            Ok(Value::str(&s))
        }
        "list.group_by" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = match &args[1] {
                Value::Fn(c) => c.clone(),
                other => return mismatch(name, "a function", other),
            };
            let fal = it.cb_fallible(&c);
            let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
            for x in xs.iter() {
                let k = it.call_value(&c, vec![x.clone()])?;
                let k = if fal {
                    match k {
                        Value::Ok(v) => (*v).clone(),
                        Value::Err(e) => return Ok(Ok(Value::Err(e))),
                        other => other,
                    }
                } else {
                    k
                };
                match groups
                    .iter_mut()
                    .find(|(k2, _)| values_eq(k2, &k) == Some(true))
                {
                    Some((_, v)) => v.push(x.clone()),
                    None => groups.push((k, vec![x.clone()])),
                }
            }
            let out = Value::Map(Rc::new(
                groups
                    .into_iter()
                    .map(|(k, v)| (k, Value::List(Rc::new(v))))
                    .collect(),
            ));
            Ok(if fal { ok(out) } else { out })
        }

        _ => return Err(Flow::Fatal("__not_ext__".into())),
    })
}

/// split into lines like fs.read_lines / fold_lines observe them
pub fn split_lines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c == '\n' {
            if cur.ends_with('\r') {
                cur.pop();
            }
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        if cur.ends_with('\r') {
            cur.pop();
        }
        out.push(cur);
    }
    out
}

fn read_bytes_fn(it: &Interp, name: &str, args: &[Value]) -> Result<Result<Value, Flow>, Flow> {
    let _ = it;
    if name.ends_with("_array") {
        arity(name, args, 3)?;
    } else {
        arity(name, args, 2)?;
    }
    let b = want_bytes(name, &args[0])?;
    let b = b.borrow();
    let pos = want_int(name, &args[1])?;
    let (width, be): (usize, bool) = match name.trim_start_matches("bytes.read_") {
        "u8" => (1, false),
        n if n.starts_with("u16") || n.starts_with("i16") || n.starts_with("f16") => {
            (2, n.ends_with("be"))
        }
        n if n.starts_with("u32") || n.starts_with("i32") || n.starts_with("f32") => {
            (4, n.ends_with("be"))
        }
        n if n.starts_with("u64") || n.starts_with("i64") || n.starts_with("f64") => {
            (8, n.ends_with("be"))
        }
        _ => return Err(Flow::Fatal("__not_ext__".into())),
    };
    let count = if name.ends_with("_array") {
        want_int(name, &args[2])?.max(0)
    } else {
        1
    };
    if count > (1i64 << 31) {
        return Ok(Err(Flow::Abort("out of memory".into())));
    }
    let count = count as usize;
    let mut vals: Vec<Value> = Vec::with_capacity(count);
    // TOTAL reads, per VALUE: a window fully in range reads its bytes; any
    // window that is not — negative offset, past the end, or PARTIAL
    // overlap — reads as a whole 0 (bytes_negative_offset_family,
    // bytes_f16_offset_overflow: the partial f16 is 0.0, not a hybrid)
    for k in 0..count {
        let off = pos as i128 + (k * width) as i128;
        let in_range = off >= 0 && off + width as i128 <= b.len() as i128;
        let raw: u64 = if !in_range {
            0
        } else {
            let mut r: u64 = 0;
            for j in 0..width {
                let x = b[off as usize + j] as u64;
                if be {
                    r = (r << 8) | x;
                } else {
                    r |= x << (8 * j);
                }
            }
            r
        };
        let base = name.trim_start_matches("bytes.read_");
        vals.push(if base.starts_with("f16") {
            Value::Float(F64(f16_to_f64(raw as u16)))
        } else if base.starts_with("f32") {
            Value::Float(F64(f32::from_bits(raw as u32) as f64))
        } else if base.starts_with("f64") {
            Value::Float(F64(f64::from_bits(raw)))
        } else if base.starts_with("i16") {
            Value::Int(raw as u16 as i16 as i64)
        } else if base.starts_with("i32") {
            Value::Int(raw as u32 as i32 as i64)
        } else {
            // i64 sign-carries through the u64 bits; u8/u16/u32 zero-extend
            Value::Int(raw as i64)
        });
    }
    Ok(Ok(if name.ends_with("_array") {
        Value::List(Rc::new(vals))
    } else {
        vals.pop().unwrap_or(Value::Int(0))
    }))
}

fn write_bytes_fn(it: &Interp, name: &str, args: &[Value]) -> Result<Result<Value, Flow>, Flow> {
    let _ = it;
    let is_set = name.starts_with("bytes.set_");
    arity(name, args, if is_set { 3 } else { 2 })?;
    let b = want_bytes(name, &args[0])?;
    let base = if is_set {
        name.trim_start_matches("bytes.set_")
    } else {
        name.trim_start_matches("bytes.append_")
    };
    let (width, be): (usize, bool) = match base {
        "u8" => (1, false),
        n if n.starts_with("u16") || n.starts_with("i16") => (2, n.ends_with("be")),
        n if n.starts_with("u32") || n.starts_with("i32") || n.starts_with("f32") => {
            (4, n.ends_with("be"))
        }
        n if n.starts_with("u64") || n.starts_with("i64") || n.starts_with("f64") => {
            (8, n.ends_with("be"))
        }
        _ => return Err(Flow::Fatal("__not_ext__".into())),
    };
    let val_at = if is_set { 2 } else { 1 };
    let raw: u64 = if base.starts_with("f32") {
        (want_float(name, &args[val_at])? as f32).to_bits() as u64
    } else if base.starts_with("f64") {
        want_float(name, &args[val_at])?.to_bits()
    } else {
        want_int(name, &args[val_at])? as u64
    };
    let mut chunk = vec![0u8; width];
    for (j, slot) in chunk.iter_mut().enumerate() {
        let shift = if be { 8 * (width - 1 - j) } else { 8 * j };
        *slot = (raw >> shift) as u8;
    }
    let mut v = b.borrow_mut();
    if is_set {
        // OOB set_* is a silent NO-OP (bytes_writer_family set_past_end,
        // bytes_negative_offset_family)
        let pos = want_int(name, &args[1])?;
        if pos >= 0 && (pos as u64).saturating_add(width as u64) <= v.len() as u64 {
            let p = pos as usize;
            v[p..p + width].copy_from_slice(&chunk);
        }
    } else {
        v.extend_from_slice(&chunk);
    }
    Ok(Ok(Value::Unit))
}

/// IEEE-754 binary16 → binary64, exact (ALS-D5: subnormals, ±inf, NaN, ±0)
pub(crate) fn f16_to_f64(h: u16) -> f64 {
    let sign = (h >> 15) as u64;
    let exp = ((h >> 10) & 0x1F) as i64;
    let frac = (h & 0x3FF) as u64;
    let bits: u64 = if exp == 0x1F {
        // inf / nan
        (sign << 63) | (0x7FFu64 << 52) | if frac != 0 { 1u64 << 51 } else { 0 }
    } else if exp == 0 {
        if frac == 0 {
            sign << 63
        } else {
            // subnormal: value = frac * 2^-24
            let mut f = frac;
            let mut e: i64 = -24;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            f &= 0x3FF;
            let biased = (e + 10 + 1023) as u64;
            (sign << 63) | (biased << 52) | (f << 42)
        }
    } else {
        let biased = (exp - 15 + 1023) as u64;
        (sign << 63) | (biased << 52) | (frac << 42)
    };
    f64::from_bits(bits)
}

/// serde_json-style pretty text (D6): two-space indent, space after colon
fn pretty(d: &Dyn, level: usize) -> String {
    let pad = "  ".repeat(level + 1);
    let close = "  ".repeat(level);
    match d {
        Dyn::A(items) if !items.is_empty() => {
            let mut out = String::from("[\n");
            for (i, x) in items.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(&pretty(x, level + 1));
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&close);
            out.push(']');
            out
        }
        Dyn::O(fields) if !fields.is_empty() => {
            let mut out = String::from("{\n");
            for (i, (k, v)) in fields.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(&json_quote(k));
                out.push_str(": ");
                out.push_str(&pretty(v, level + 1));
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&close);
            out.push('}');
            out
        }
        other => dyn_text(other),
    }
}

// ── json.parse — transcribed from stdlib/json_parse.almd (the NORMATIVE
// self-hosted parser, ALS-T3): byte-level, deliberately lenient — strings
// never fail (EOF closes them, unknown escapes and invalid \uXXXX drop
// silently, surrogate pairs join), `:` and separators are optional exactly
// where the oracle is lenient, trailing input after the first value is
// ignored, error positions are CHAR indices. ──

fn json_parse(text: &str) -> Value {
    let b = text.as_bytes();
    match jp_value(b, 0) {
        Ok((v, _p)) => Value::Ok(Rc::new(Value::Dyn(v))),
        Err(m) => err_str(&m),
    }
}

fn jp_is_uws(c: u32) -> bool {
    matches!(c, 32 | 9..=13 | 133 | 160 | 5760 | 8192..=8202 | 8232 | 8233 | 8239 | 8287 | 12288)
}

/// decode the UTF-8 codepoint at p → (codepoint, byte length)
fn jp_cp(b: &[u8], p: usize) -> (u32, usize) {
    let b0 = b[p] as u32;
    let at = |i: usize| -> u32 { b.get(p + i).copied().unwrap_or(0) as u32 };
    if b0 < 128 {
        (b0, 1)
    } else if b0 < 224 {
        ((b0 - 192) * 64 + (at(1) - 128), 2)
    } else if b0 < 240 {
        ((b0 - 224) * 4096 + (at(1) - 128) * 64 + (at(2) - 128), 3)
    } else {
        (
            (b0 - 240) * 262144 + (at(1) - 128) * 4096 + (at(2) - 128) * 64 + (at(3) - 128),
            4,
        )
    }
}

fn jp_charpos(b: &[u8], p: usize) -> usize {
    b[..p.min(b.len())]
        .iter()
        .filter(|&&x| !(128..192).contains(&x))
        .count()
}

fn jp_ws(b: &[u8], mut p: usize) -> usize {
    while p < b.len() {
        let x = b[p];
        if x == 32 || (9..=13).contains(&x) {
            p += 1;
        } else if x >= 194 {
            let (cp, l) = jp_cp(b, p);
            if jp_is_uws(cp) {
                p += l;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    p
}

fn jp_value(b: &[u8], p: usize) -> Result<(Dyn, usize), String> {
    let q = jp_ws(b, p);
    if q >= b.len() {
        return Err("unexpected end of input".into());
    }
    match b[q] {
        34 => {
            let (s, np) = jp_string(b, q);
            Ok((Dyn::S(Rc::from(s.as_str())), np))
        }
        123 => {
            let q2 = jp_ws(b, q + 1);
            if q2 < b.len() && b[q2] == 125 {
                Ok((Dyn::O(Rc::new(Vec::new())), q2 + 1))
            } else {
                jp_object(b, q2)
            }
        }
        91 => {
            let q2 = jp_ws(b, q + 1);
            if q2 < b.len() && b[q2] == 93 {
                Ok((Dyn::A(Rc::new(Vec::new())), q2 + 1))
            } else {
                jp_array(b, q2)
            }
        }
        116 | 102 => jp_bool(b, q),
        110 => {
            if b.len() >= q + 4 && &b[q..q + 4] == b"null" {
                Ok((Dyn::Null, q + 4))
            } else {
                Err("expected null".into())
            }
        }
        45 | 48..=57 => jp_number(b, q),
        _ => {
            let (cp, _l) = jp_cp(b, q);
            let shown = char::from_u32(cp)
                .map(|c| c.to_string())
                .unwrap_or_default();
            Err(format!(
                "unexpected char '{shown}' at pos {}",
                jp_charpos(b, q)
            ))
        }
    }
}

/// parse_string never fails: p is at the opening quote (consumed
/// unconditionally); returns (decoded, position after the closing quote/EOF)
fn jp_string(b: &[u8], p: usize) -> (String, usize) {
    let mut out: Vec<u8> = Vec::new();
    let mut p = p + 1;
    loop {
        if p >= b.len() {
            break;
        }
        match b[p] {
            34 => {
                p += 1;
                break;
            }
            92 => {
                let e = b.get(p + 1).copied().unwrap_or(0);
                match e {
                    110 => {
                        out.push(10);
                        p += 2;
                    }
                    116 => {
                        out.push(9);
                        p += 2;
                    }
                    114 => {
                        out.push(13);
                        p += 2;
                    }
                    98 => {
                        out.push(8);
                        p += 2;
                    }
                    102 => {
                        out.push(12);
                        p += 2;
                    }
                    34 | 92 | 47 => {
                        out.push(e);
                        p += 2;
                    }
                    117 => {
                        // \uXXXX: advance past the 4 hex digits UNCONDITIONALLY;
                        // write only when valid; surrogate pairs join; lone or
                        // invalid units drop
                        let unit = jp_hex4(b, p + 2);
                        if unit < 0 {
                            p += 6;
                        } else if (55296..=56319).contains(&unit)
                            && p + 8 <= b.len()
                            && b.get(p + 6) == Some(&92)
                            && b.get(p + 7) == Some(&117)
                        {
                            let lo = jp_hex4(b, p + 8);
                            if (56320..=57343).contains(&lo) {
                                let cp = 65536 + (unit - 55296) * 1024 + (lo - 56320);
                                jp_utf8_push(&mut out, cp as u32);
                                p += 12;
                            } else {
                                p += 6;
                            }
                        } else if (55296..=57343).contains(&unit) {
                            p += 6;
                        } else {
                            jp_utf8_push(&mut out, unit as u32);
                            p += 6;
                        }
                    }
                    _ => {
                        p += 2; // unknown escape: dropped
                    }
                }
            }
            x => {
                out.push(x);
                p += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).to_string(), p)
}

fn jp_utf8_push(out: &mut Vec<u8>, cp: u32) {
    if cp < 128 {
        out.push(cp as u8);
    } else if cp < 2048 {
        out.push(192 + (cp / 64) as u8);
        out.push(128 + (cp % 64) as u8);
    } else if cp < 65536 {
        out.push(224 + (cp / 4096) as u8);
        out.push(128 + ((cp / 64) % 64) as u8);
        out.push(128 + (cp % 64) as u8);
    } else {
        out.push(240 + (cp / 262144) as u8);
        out.push(128 + ((cp / 4096) % 64) as u8);
        out.push(128 + ((cp / 64) % 64) as u8);
        out.push(128 + (cp % 64) as u8);
    }
}

fn jp_hex4(b: &[u8], p: usize) -> i64 {
    if p + 4 > b.len() {
        return -1;
    }
    let mut v: i64 = 0;
    for i in 0..4 {
        let d = match b[p + i] {
            x @ 48..=57 => (x - 48) as i64,
            x @ 97..=102 => (x - 87) as i64,
            x @ 65..=70 => (x - 55) as i64,
            _ => return -1,
        };
        v = v * 16 + d;
    }
    v
}

fn jp_digits(b: &[u8], mut p: usize) -> usize {
    while p < b.len() && b[p].is_ascii_digit() {
        p += 1;
    }
    p
}

fn jp_number(b: &[u8], p: usize) -> Result<(Dyn, usize), String> {
    let p1 = if b[p] == 45 { p + 1 } else { p };
    let p2 = jp_digits(b, p1);
    let has_frac = p2 < b.len() && b[p2] == 46;
    let p3 = if has_frac { jp_digits(b, p2 + 1) } else { p2 };
    let eb = b.get(p3).copied().unwrap_or(0);
    let has_exp = eb == 101 || eb == 69;
    let p4 = if has_exp {
        let ps = p3 + 1;
        let sb = b.get(ps).copied().unwrap_or(0);
        jp_digits(b, if sb == 43 || sb == 45 { ps + 1 } else { ps })
    } else {
        p3
    };
    let s = String::from_utf8_lossy(&b[p..p4]).to_string();
    if has_frac || has_exp {
        // the oracle routes through float.parse (ALS-T2) and passes its
        // error text through
        match crate::stdlib::float_parse_t2(&s) {
            Ok(f) => Ok((Dyn::F(f), p4)),
            Err(m) => Err(m),
        }
    } else {
        match crate::stdlib::int_parse_t8(&s) {
            Ok(n) => Ok((Dyn::I(n), p4)),
            Err(m) => Err(m),
        }
    }
}

fn jp_bool(b: &[u8], p: usize) -> Result<(Dyn, usize), String> {
    if b.len() >= p + 4 && &b[p..p + 4] == b"true" {
        Ok((Dyn::B(true), p + 4))
    } else if b.len() >= p + 5 && &b[p..p + 5] == b"false" {
        Ok((Dyn::B(false), p + 5))
    } else {
        Err("expected bool".into())
    }
}

fn jp_array(b: &[u8], p0: usize) -> Result<(Dyn, usize), String> {
    let mut p = p0;
    let mut acc: Vec<Dyn> = Vec::new();
    loop {
        let (v, np) = jp_value(b, p)?;
        acc.push(v);
        let q = jp_ws(b, np);
        let x = b.get(q).copied().unwrap_or(0);
        if x == 44 {
            p = q + 1;
        } else if x == 93 {
            return Ok((Dyn::A(Rc::new(acc)), q + 1));
        } else {
            return Ok((Dyn::A(Rc::new(acc)), q));
        }
    }
}

fn jp_object(b: &[u8], p0: usize) -> Result<(Dyn, usize), String> {
    let mut p = p0;
    let mut acc: Vec<(Rc<str>, Dyn)> = Vec::new();
    loop {
        let q = jp_ws(b, p);
        let (k, kp) = jp_string(b, q);
        let q2 = jp_ws(b, kp);
        let q3 = if q2 < b.len() && b[q2] == 58 {
            q2 + 1
        } else {
            q2
        };
        let (v, np) = jp_value(b, q3)?;
        let kr: Rc<str> = Rc::from(k.as_str());
        match acc.iter_mut().find(|(k2, _)| *k2 == kr) {
            Some(slot) => slot.1 = v,
            None => acc.push((kr, v)),
        }
        let q4 = jp_ws(b, np);
        let x = b.get(q4).copied().unwrap_or(0);
        if x == 44 {
            p = q4 + 1;
        } else if x == 125 {
            return Ok((Dyn::O(Rc::new(acc)), q4 + 1));
        } else {
            return Ok((Dyn::O(Rc::new(acc)), q4));
        }
    }
}

// keep the unused-import lint quiet if Ordering falls out of use later
#[allow(dead_code)]
fn _o(_: Ordering) {}
