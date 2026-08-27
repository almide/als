//! Round-3 stdlib module: `matrix` — the f64 matrix value, its constructors,
//! the byte-buffer loaders/selectors (f32 LE, fp16, Q1_0, Q8_0) and the
//! index/reduction domain. Written from C-161 (dims normalize + the 2^28
//! ceiling), C-223 (transcendentals through the vendored musl-libm),
//! C-228/C-229 (selector decode + OOB→zeros/clamps), C-270 (the shared fp16
//! block scale, +0.0 dequant ruling), C-282 (`matrix.get` aborts out of
//! range) and the fixtures they cite; unknown edges abstain with a class.
//!
//! Sign discipline (C-270): a DEQUANTIZED element is written as `0.0 + v`,
//! the accumulate-into-zeros spelling that lands every zero-magnitude
//! product on +0.0 while leaving infinities, NaN and normal values alone.
//! The float DECODERS (`from_bytes_f16_le` / `from_bytes_f32_le` /
//! `select_rows_f32`) store the datum verbatim — a stored -0.0 is
//! information there.

use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::stdlib_ext::f16_to_f64;
use crate::value::{Mat, Value, F64};

pub const MATRIX_FNS: &[&str] = &[
    "matrix.zeros",
    "matrix.ones",
    "matrix.shape",
    "matrix.rows",
    "matrix.get",
    "matrix.from_lists",
    "matrix.to_lists",
    "matrix.row_dot",
    "matrix.dot_row",
    "matrix.pow",
    "matrix.gelu",
    "matrix.rms_norm_rows",
    "matrix.select_rows_f32",
    "matrix.select_rows_q8_0_dq",
    "matrix.select_rows_q1_0",
    "matrix.from_q1_0_bytes",
    "matrix.from_bytes_f32_le",
    "matrix.from_bytes_f16_le",
];

/// C-161: 2^28 elements (2 GiB of f64, inside wasm32's address space), and
/// the same ceiling on the ROW count alone (matrix_dims_guard_rows).
const DIM_CEILING: i64 = 1 << 28;

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

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

fn want_float(name: &str, v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(F64(f)) => Ok(*f),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Float, got {}",
            other.type_name()
        ))),
    }
}

fn want_mat(name: &str, v: &Value) -> Result<Rc<Mat>, Flow> {
    match v {
        Value::Matrix(m) => Ok(m.clone()),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Matrix, got {}",
            other.type_name()
        ))),
    }
}

fn want_bytes(name: &str, v: &Value) -> Result<Vec<u8>, Flow> {
    match v {
        Value::Bytes(b) => Ok(b.borrow().clone()),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Bytes, got {}",
            other.type_name()
        ))),
    }
}

/// C-161 normalization: negatives clamp to 0 (a zero row count empties the
/// matrix — `ones(-2^31, 16)` answers 0 x 0), then the shared ceiling aborts
/// in the T6 form.
fn norm_dims(r: i64, c: i64) -> Result<(i64, i64), Flow> {
    let r = r.max(0);
    let mut c = c.max(0);
    if r == 0 {
        c = 0;
    }
    if r > DIM_CEILING || (r as i128) * (c as i128) > DIM_CEILING as i128 {
        return Err(Flow::Abort("matrix dimensions too large".into()));
    }
    Ok((r, c))
}

fn mat(rows: i64, cols: i64, data: Vec<f64>) -> Value {
    Value::Matrix(Rc::new(Mat { rows, cols, data }))
}

fn scale_at(data: &[u8], start: usize) -> f64 {
    f16_to_f64(u16::from_le_bytes([data[start], data[start + 1]]))
}

pub fn call_matrix(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !name.starts_with("matrix.") || !MATRIX_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(it, name, args))
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        "matrix.zeros" | "matrix.ones" => {
            arity(name, &args, 2)?;
            let (r, c) = norm_dims(want_int(name, &args[0])?, want_int(name, &args[1])?)?;
            let fill = if name == "matrix.ones" { 1.0 } else { 0.0 };
            Ok(mat(r, c, vec![fill; (r * c) as usize]))
        }
        "matrix.shape" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            Ok(Value::Tuple(Rc::new(vec![
                Value::Int(m.rows),
                Value::Int(m.cols),
            ])))
        }
        "matrix.rows" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_mat(name, &args[0])?.rows))
        }
        "matrix.get" => {
            arity(name, &args, 3)?;
            let m = want_mat(name, &args[0])?;
            let r = want_int(name, &args[1])?;
            let c = want_int(name, &args[2])?;
            // C-282: indexing has no identity value — out of range ABORTS
            // like `xs[i]`, negatives included
            if r < 0 || r >= m.rows || c < 0 || c >= m.cols {
                return Err(Flow::Abort("matrix index out of bounds".into()));
            }
            Ok(Value::Float(F64(m.data[(r * m.cols + c) as usize])))
        }
        "matrix.from_lists" => {
            arity(name, &args, 1)?;
            let rows_v = match &args[0] {
                Value::List(xs) => xs.clone(),
                other => {
                    return Err(Flow::Fatal(format!(
                        "{name}: expected List, got {}",
                        other.type_name()
                    )))
                }
            };
            let mut data: Vec<f64> = Vec::new();
            let mut cols: i64 = -1;
            for row in rows_v.iter() {
                let Value::List(cells) = row else {
                    return it.abstain_pub(
                        "stdlib:matrix.from_lists",
                        format!("a {} row in matrix.from_lists", row.type_name()),
                    );
                };
                if cols < 0 {
                    cols = cells.len() as i64;
                } else if cols != cells.len() as i64 {
                    return it.abstain_pub(
                        "stdlib:matrix.from_lists",
                        "jagged rows in matrix.from_lists",
                    );
                }
                for cell in cells.iter() {
                    match cell {
                        Value::Float(F64(f)) => data.push(*f),
                        other => {
                            return it.abstain_pub(
                                "stdlib:matrix.from_lists",
                                format!("a {} element in matrix.from_lists", other.type_name()),
                            )
                        }
                    }
                }
            }
            let (r, c) = norm_dims(rows_v.len() as i64, cols.max(0))?;
            data.truncate((r * c) as usize);
            Ok(mat(r, c, data))
        }
        "matrix.to_lists" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            let mut rows = Vec::with_capacity(m.rows as usize);
            for r in 0..m.rows {
                let mut row = Vec::with_capacity(m.cols as usize);
                for c in 0..m.cols {
                    row.push(Value::Float(F64(m.data[(r * m.cols + c) as usize])));
                }
                rows.push(Value::List(Rc::new(row)));
            }
            Ok(Value::List(Rc::new(rows)))
        }
        "matrix.row_dot" | "matrix.dot_row" => {
            arity(name, &args, 3)?;
            let m = want_mat(name, &args[0])?;
            let r = want_int(name, &args[1])?;
            let ws = match &args[2] {
                Value::List(xs) => xs.clone(),
                other => {
                    return Err(Flow::Fatal(format!(
                        "{name}: expected List, got {}",
                        other.type_name()
                    )))
                }
            };
            // C-282's other half: a REDUCTION has the empty-sum identity, so
            // an out-of-range (or negative) row answers 0.0 and never aborts
            if r < 0 || r >= m.rows {
                return Ok(Value::Float(F64(0.0)));
            }
            if ws.len() as i64 != m.cols {
                return it.abstain_pub(
                    &format!("stdlib:{name}"),
                    format!("a {}-element vector against {} columns", ws.len(), m.cols),
                );
            }
            let mut acc = 0.0f64;
            for (j, w) in ws.iter().enumerate() {
                acc += m.data[(r * m.cols) as usize + j] * want_float(name, w)?;
            }
            Ok(Value::Float(F64(acc)))
        }
        "matrix.pow" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let e = want_float(name, &args[1])?;
            // C-223: through the vendored musl-libm on both legs
            let data = m
                .data
                .iter()
                .map(|x| crate::libm::almide_rt_libm_pow(*x, e))
                .collect();
            Ok(mat(m.rows, m.cols, data))
        }
        "matrix.gelu" => {
            arity(name, &args, 1)?;
            let _ = want_mat(name, &args[0])?;
            // C-223: gelu computes through the CANONICAL degree-6 fast-exp
            // (`1 - 2/(exp(2y)+1)`), whose coefficients the judge does not
            // pin yet — a libm tanh is bit-different in the 8th digit, so
            // abstaining is the only honest answer until the algorithm lands
            it.abstain_pub(
                "stdlib:matrix.gelu",
                "the canonical deg-6 fast-exp is not pinned in the judge yet",
            )
        }
        "matrix.rms_norm_rows" => {
            arity(name, &args, 3)?;
            let m = want_mat(name, &args[0])?;
            if m.rows == 0 {
                // the empty matrix normalizes to itself (matrix_dims_guard);
                // the non-empty kernel is not implemented yet
                return Ok(mat(m.rows, m.cols, Vec::new()));
            }
            it.abstain_pub(
                "stdlib:matrix.rms_norm_rows",
                "rms_norm_rows over a non-empty matrix is not implemented yet",
            )
        }
        "matrix.select_rows_f32" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?.max(0);
            let cols = want_int(name, &args[2])?.max(0);
            let rids = want_rids(name, &args[3])?;
            let mut data = vec![0.0f64; rids.len() * cols as usize];
            for (i, rid) in rids.iter().enumerate() {
                // C-229: a negative row id clamps to row 0; a row whose byte
                // range leaves the buffer is the all-zero row
                let rid = (*rid).max(0);
                let start = off as i128 + rid as i128 * cols as i128 * 4;
                let end = start + cols as i128 * 4;
                if start < 0 || end > b.len() as i128 {
                    continue;
                }
                for c in 0..cols as usize {
                    let p = start as usize + c * 4;
                    let raw = u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
                    data[i * cols as usize + c] = f32::from_bits(raw) as f64;
                }
            }
            Ok(mat(rids.len() as i64, cols, data))
        }
        "matrix.select_rows_q8_0_dq" | "matrix.select_rows_q1_0" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?.max(0);
            let cols = want_int(name, &args[2])?.max(0);
            let rids = want_rids(name, &args[3])?;
            let q8 = name == "matrix.select_rows_q8_0_dq";
            let mut data = vec![0.0f64; rids.len() * cols as usize];
            for (i, rid) in rids.iter().enumerate() {
                let rid = (*rid).max(0);
                for c in 0..cols {
                    let k = rid as i128 * cols as i128 + c as i128;
                    data[i * cols as usize + c as usize] = q_element(&b, off, k, q8);
                }
            }
            Ok(mat(rids.len() as i64, cols, data))
        }
        "matrix.from_q1_0_bytes" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?.max(0);
            let (r, c) = norm_dims(want_int(name, &args[2])?, want_int(name, &args[3])?)?;
            let mut data = vec![0.0f64; (r * c) as usize];
            for row in 0..r {
                for col in 0..c {
                    let k = row as i128 * c as i128 + col as i128;
                    data[(row * c + col) as usize] = q_element(&b, off, k, false);
                }
            }
            Ok(mat(r, c, data))
        }
        "matrix.from_bytes_f32_le" | "matrix.from_bytes_f16_le" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?.max(0);
            let (r, c) = norm_dims(want_int(name, &args[2])?, want_int(name, &args[3])?)?;
            let w = if name == "matrix.from_bytes_f32_le" {
                4
            } else {
                2
            };
            let mut data = vec![0.0f64; (r * c) as usize];
            for row in 0..r {
                let start = off as i128 + row as i128 * c as i128 * w;
                let end = start + c as i128 * w;
                if start < 0 || end > b.len() as i128 {
                    continue; // the all-zero row (C-229's edge on the full loaders)
                }
                for col in 0..c as usize {
                    let p = start as usize + col * w as usize;
                    // a DECODER, not a dequantizer: the stored sign survives
                    data[(row * c) as usize + col] = if w == 4 {
                        f32::from_bits(u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]))
                            as f64
                    } else {
                        f16_to_f64(u16::from_le_bytes([b[p], b[p + 1]]))
                    };
                }
            }
            Ok(mat(r, c, data))
        }
        other => Err(Flow::Fatal(format!("unrouted matrix fn `{other}`"))),
    }
}

fn want_rids(name: &str, v: &Value) -> Result<Vec<i64>, Flow> {
    match v {
        Value::List(xs) => xs.iter().map(|x| want_int(name, x)).collect(),
        other => Err(Flow::Fatal(format!(
            "{name}: expected List of Int, got {}",
            other.type_name()
        ))),
    }
}

/// One dequantized element on the GLOBAL-k block schedule (C-270,
/// matrix_q_dims_guard): element k reads block k>>5 (Q8_0: 34-byte blocks,
/// 32 weights) or k>>7 (Q1_0: 18-byte blocks, 128 sign bits, LSB-first,
/// bit 1 → +scale / bit 0 → -scale), with a PER-ELEMENT bound — a block
/// outside the data region is 0.0, never a read past it. Written `0.0 + v`:
/// the accumulate-into-zeros sign ruling.
fn q_element(b: &[u8], off: i64, k: i128, q8: bool) -> f64 {
    let (block_bytes, weights_per_block) = if q8 {
        (34i128, 32i128)
    } else {
        (18i128, 128i128)
    };
    let block = k / weights_per_block;
    let start = off as i128 + block * block_bytes;
    if start < 0 || start + block_bytes > b.len() as i128 {
        return 0.0;
    }
    let start = start as usize;
    let scale = scale_at(b, start);
    let w = (k % weights_per_block) as usize;
    let v = if q8 {
        let q = b[start + 2 + w] as i8;
        scale * q as f64
    } else if (b[start + 2 + (w >> 3)] >> (w & 7)) & 1 == 1 {
        scale
    } else {
        0.0 - scale
    };
    0.0 + v
}
