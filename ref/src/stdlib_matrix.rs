//! Round-3 stdlib module: `matrix` — the f64 matrix value, its constructors,
//! the byte-buffer loaders/selectors (f32 LE, fp16) and the
//! index/reduction domain. Written from C-161 (dims normalize + the 2^28
//! ceiling), C-223 (transcendentals through the vendored musl-libm),
//! C-228/C-229 (selector decode + OOB→zeros/clamps), C-282 (`matrix.get`
//! answers `none` out of range), C-341 (the byte loaders answer `none` when
//! the data does not fit) and the fixtures they cite; unknown edges abstain
//! with a class.
//!
//! Sign discipline: the float DECODERS (`from_bytes_f16_le` /
//! `from_bytes_f32_le` / `from_bytes_f64_le` / `select_rows_f32`) store the
//! datum verbatim — a stored -0.0 is information there.

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
    "matrix.select_rows_f32",
    "matrix.from_bytes_f32_le",
    "matrix.from_bytes_f16_le",
    "matrix.from_bytes_f64_le",
    // C-353 / C-354 / C-355: the shape-precondition, row-range and
    // signed-zero rules, and the exact-arithmetic kernels they govern.
    "matrix.cols",
    "matrix.transpose",
    "matrix.neg",
    "matrix.scale",
    "matrix.map",
    "matrix.add",
    "matrix.sub",
    "matrix.div",
    "matrix.broadcast_add_row",
    "matrix.mul",
    "matrix.linear_row",
    "matrix.linear_row_no_bias",
    "matrix.slice_rows",
    "matrix.split_cols_even",
    "matrix.concat_cols",
    "matrix.concat_cols_many",
    "matrix.conv1d",
    // C-161's composition family (stdlib/matrix_fused.almd in the
    // implementation): each is a composition of the kernels above, so it is
    // written here as that composition — fma = add∘scale (zip shape, two
    // rounded products and a rounded sum, never fused), mul_scaled =
    // scale∘mul, the `_f32` names alias the f64 arithmetic.
    "matrix.fma",
    "matrix.fma3",
    "matrix.mul_scaled",
    "matrix.mul_f32",
    "matrix.mul_f32_scaled",
    "matrix.mul_f32_t",
    "matrix.mul_f32_t_scaled",
    "matrix.zeros_f32",
    "matrix.ones_f32",
    "matrix.append_rows",
    "matrix.linear_f32_row_no_bias",
    "matrix.to_bytes_f32_le",
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

/// `matrix.cols` / `matrix.shape` answer ROW 0's width, which is 0 when the
/// matrix has no rows — both implementations read the first row rather than
/// a stored width, so a 0-row result of `mul(empty, 2x2)` answers 0 columns
/// however wide the multiplication would have been.
fn observable_cols(m: &Mat) -> i64 {
    if m.rows == 0 {
        0
    } else {
        m.cols
    }
}

/// C-353: two extents a kernel indexes against each other must be EQUAL,
/// or the call aborts in the unified T6 form.
fn shape_eq(a: i64, b: i64) -> Result<(), Flow> {
    if a == b {
        Ok(())
    } else {
        Err(Flow::Abort("matrix shape mismatch".into()))
    }
}

fn want_floats(name: &str, v: &Value) -> Result<Vec<f64>, Flow> {
    match v {
        Value::List(xs) => xs.iter().map(|x| want_float(name, x)).collect(),
        other => Err(Flow::Fatal(format!(
            "{name}: expected List[Float], got {}",
            other.type_name()
        ))),
    }
}

fn mismatch_mat(name: &str, other: &Value) -> Result<Value, Flow> {
    Err(Flow::Fatal(format!(
        "{name}: expected List[Matrix], got {}",
        other.type_name()
    )))
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
                Value::Int(observable_cols(&m)),
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
            // C-282: `Option[Float]` — out of range (negatives included) is
            // `none`; compared as i64 before any cast
            if r < 0 || r >= m.rows || c < 0 || c >= m.cols {
                return Ok(Value::None);
            }
            Ok(Value::Some(Rc::new(Value::Float(F64(
                m.data[(r * m.cols + c) as usize]
            )))))
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
                    // C-353: a ragged list is not a matrix — every row must
                    // have the FIRST row's width, and the constructor aborts
                    // rather than inventing a shape. This is what makes
                    // C-282's "no public constructor builds a ragged matrix"
                    // true (almide/almide#2482).
                    return Err(Flow::Abort("matrix rows must have equal length".into()));
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
        "matrix.cols" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            Ok(Value::Int(observable_cols(&m)))
        }
        "matrix.transpose" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            if m.rows == 0 || m.cols == 0 {
                return Ok(mat(0, 0, Vec::new()));
            }
            let mut data = Vec::with_capacity(m.data.len());
            for c in 0..m.cols {
                for r in 0..m.rows {
                    data.push(m.data[(r * m.cols + c) as usize]);
                }
            }
            Ok(mat(m.cols, m.rows, data))
        }
        // C-355: negation is a SIGN FLIP, so neg(+0.0) is -0.0 — `0.0 - x`
        // would answer +0.0 for a zero element (ALS-T23).
        "matrix.neg" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            Ok(mat(m.rows, m.cols, m.data.iter().map(|x| -x).collect()))
        }
        "matrix.scale" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let s = want_float(name, &args[1])?;
            Ok(mat(m.rows, m.cols, m.data.iter().map(|x| x * s).collect()))
        }
        "matrix.map" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let c = match &args[1] {
                Value::Fn(c) => c.clone(),
                other => {
                    return Err(Flow::Fatal(format!(
                        "{name}: expected a function, got {}",
                        other.type_name()
                    )))
                }
            };
            let mut data = Vec::with_capacity(m.data.len());
            for x in m.data.iter() {
                let v = it.call_value(&c, vec![Value::Float(F64(*x))])?;
                data.push(want_float(name, &v)?);
            }
            Ok(mat(m.rows, m.cols, data))
        }
        // The ELEMENTWISE pair family (C-353's stated exception): rows and
        // cols ZIP-truncate to the shorter operand on every leg.
        "matrix.add" | "matrix.sub" | "matrix.div" => {
            arity(name, &args, 2)?;
            let a = want_mat(name, &args[0])?;
            let b = want_mat(name, &args[1])?;
            let rows = a.rows.min(b.rows);
            let cols = a.cols.min(b.cols);
            let mut data = Vec::with_capacity((rows * cols) as usize);
            for r in 0..rows {
                for c in 0..cols {
                    let (x, y) = (
                        a.data[(r * a.cols + c) as usize],
                        b.data[(r * b.cols + c) as usize],
                    );
                    data.push(match name {
                        "matrix.add" => x + y,
                        "matrix.sub" => x - y,
                        _ => x / y,
                    });
                }
            }
            Ok(mat(rows, cols, data))
        }
        "matrix.broadcast_add_row" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let bias = want_floats(name, &args[1])?;
            let cols = m.cols.min(bias.len() as i64);
            let mut data = Vec::with_capacity((m.rows * cols) as usize);
            for r in 0..m.rows {
                for c in 0..cols {
                    data.push(m.data[(r * m.cols + c) as usize] + bias[c as usize]);
                }
            }
            Ok(mat(m.rows, cols, data))
        }
        // C-353: the inner dimension is a PRECONDITION. The empty
        // short-circuit answers first (C-278's empty-operand rule).
        "matrix.mul" => {
            arity(name, &args, 2)?;
            let a = want_mat(name, &args[0])?;
            let b = want_mat(name, &args[1])?;
            let (m, k, n) = (a.rows, a.cols, b.cols);
            if m == 0 || k == 0 || n == 0 {
                return Ok(mat(m, n, vec![0.0; (m * n) as usize]));
            }
            shape_eq(k, b.rows)?;
            let mut data = vec![0.0f64; (m * n) as usize];
            for i in 0..m {
                for j in 0..n {
                    // k ASCENDING from 0.0 — the accumulation order both
                    // implementations spell (matmul_naive reduces to it).
                    let mut acc = 0.0f64;
                    for kk in 0..k {
                        acc += a.data[(i * k + kk) as usize] * b.data[(kk * n + j) as usize];
                    }
                    data[(i * n + j) as usize] = acc;
                }
            }
            Ok(mat(m, n, data))
        }
        // C-353: y[i][j] = sum_k x[i][k]*w[j][k] (+ bias[j]), k ascending,
        // bias added last; cols(x) == cols(w) and len(bias) == rows(w).
        "matrix.linear_row" | "matrix.linear_row_no_bias" => {
            let with_bias = name == "matrix.linear_row";
            arity(name, &args, if with_bias { 3 } else { 2 })?;
            let x = want_mat(name, &args[0])?;
            let w = want_mat(name, &args[1])?;
            let bias = if with_bias {
                want_floats(name, &args[2])?
            } else {
                Vec::new()
            };
            if x.rows == 0 || w.rows == 0 {
                return Ok(mat(0, 0, Vec::new()));
            }
            shape_eq(w.cols, x.cols)?;
            if with_bias {
                shape_eq(bias.len() as i64, w.rows)?;
            }
            let (r, n_in, n_out) = (x.rows, x.cols, w.rows);
            let mut data = Vec::with_capacity((r * n_out) as usize);
            for i in 0..r {
                for j in 0..n_out {
                    let mut acc = 0.0f64;
                    for kk in 0..n_in {
                        acc += x.data[(i * n_in + kk) as usize] * w.data[(j * n_in + kk) as usize];
                    }
                    data.push(if with_bias {
                        acc + bias[j as usize]
                    } else {
                        acc
                    });
                }
            }
            Ok(mat(r, n_out, data))
        }
        // C-354: `list.slice` over the rows — a negative start is EMPTY, a
        // negative or past-the-end end is rows(m), start >= end is EMPTY.
        "matrix.slice_rows" => {
            arity(name, &args, 3)?;
            let m = want_mat(name, &args[0])?;
            let start = want_int(name, &args[1])?;
            let end = want_int(name, &args[2])?;
            let e = if end < 0 || end > m.rows { m.rows } else { end };
            if start < 0 || start >= e {
                return Ok(mat(0, 0, Vec::new()));
            }
            let data = m.data[(start * m.cols) as usize..(e * m.cols) as usize].to_vec();
            Ok(mat(e - start, m.cols, data))
        }
        // C-354: a part count of 0 or less is the EMPTY list; each part is
        // the `cols / n` column slice of every row.
        "matrix.split_cols_even" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let n = want_int(name, &args[1])?;
            if m.rows == 0 || n <= 0 {
                return Ok(Value::List(Rc::new(Vec::new())));
            }
            let chunk = m.cols / n;
            let mut parts = Vec::with_capacity(n as usize);
            for h in 0..n {
                let start = h * chunk;
                let mut data = Vec::with_capacity((m.rows * chunk) as usize);
                for r in 0..m.rows {
                    for c in 0..chunk {
                        data.push(m.data[(r * m.cols + start + c) as usize]);
                    }
                }
                parts.push(mat(m.rows, chunk, data));
            }
            Ok(Value::List(Rc::new(parts)))
        }
        // C-353: every NON-EMPTY member carries the first member's row
        // count; an empty member contributes no columns and no shape.
        "matrix.concat_cols" | "matrix.concat_cols_many" => {
            arity(name, &args, 1)?;
            let members = match &args[0] {
                Value::List(xs) => xs.clone(),
                other => return mismatch_mat(name, other),
            };
            let mats: Vec<Rc<Mat>> = members
                .iter()
                .map(|v| want_mat(name, v))
                .collect::<Result<_, Flow>>()?;
            if mats.is_empty() {
                return Ok(mat(0, 0, Vec::new()));
            }
            let rows0 = mats[0].rows;
            if rows0 == 0 {
                // v0's `vec![vec![]]`: ONE empty row.
                return Ok(mat(1, 0, Vec::new()));
            }
            for m in mats.iter() {
                if m.rows != 0 {
                    shape_eq(m.rows, rows0)?;
                }
            }
            let total: i64 = mats
                .iter()
                .map(|m| if m.rows == 0 { 0 } else { m.cols })
                .sum();
            let mut data = Vec::with_capacity((rows0 * total) as usize);
            for r in 0..rows0 {
                for m in mats.iter() {
                    if m.rows == 0 {
                        continue;
                    }
                    for c in 0..m.cols {
                        data.push(m.data[(r * m.cols + c) as usize]);
                    }
                }
            }
            Ok(mat(rows0, total, data))
        }
        // C-354 counts + C-353 shapes: stride is a positive STEP, kernel and
        // padding are widths that clamp at 0, the weight row is
        // `in_ch * kernel` taps and the bias one entry per output channel.
        "matrix.conv1d" => {
            arity(name, &args, 6)?;
            let input = want_mat(name, &args[0])?;
            let weight = want_mat(name, &args[1])?;
            let bias = want_floats(name, &args[2])?;
            let kernel = want_int(name, &args[3])?;
            let stride = want_int(name, &args[4])?;
            let padding = want_int(name, &args[5])?;
            let (t_in, out_ch) = (input.rows, weight.rows);
            if t_in == 0 || out_ch == 0 {
                return Ok(mat(0, 0, Vec::new()));
            }
            if stride < 1 {
                return Err(Flow::Abort("stride must be positive".into()));
            }
            let in_ch = input.cols;
            let k = kernel.max(0);
            let p = padding.max(0);
            if p > DIM_CEILING {
                return Err(Flow::Abort("matrix dimensions too large".into()));
            }
            shape_eq(weight.cols, in_ch.saturating_mul(k))?;
            shape_eq(bias.len() as i64, out_ch)?;
            let t_padded = t_in + 2 * p;
            if t_padded < k {
                return Ok(mat(0, 0, Vec::new()));
            }
            let (t_out, out_ch) = norm_dims((t_padded - k) / stride + 1, out_ch)?;
            let mut data = Vec::with_capacity((t_out * out_ch) as usize);
            for t in 0..t_out {
                let base = t * stride;
                for o in 0..out_ch {
                    let mut sum = bias[o as usize];
                    for c in 0..in_ch {
                        for ki in 0..k {
                            let tp = base + ki;
                            if tp >= p && tp < p + t_in {
                                let tc = tp - p;
                                sum += weight.data[(o * weight.cols + c * k + ki) as usize]
                                    * input.data[(tc * in_ch + c) as usize];
                            }
                        }
                    }
                    data.push(sum);
                }
            }
            Ok(mat(t_out, out_ch, data))
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
        "matrix.select_rows_f32" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?.max(0);
            let cols = want_int(name, &args[2])?.max(0);
            let rids = want_rids(name, &args[3])?;
            // C-161: the `len(ids) x cols` shape takes the shared ceiling
            // before anything is allocated
            let (_, cols) = norm_dims(rids.len() as i64, cols)?;
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
        "matrix.from_bytes_f32_le" | "matrix.from_bytes_f16_le" | "matrix.from_bytes_f64_le" => {
            arity(name, &args, 4)?;
            let b = want_bytes(name, &args[0])?;
            let off = want_int(name, &args[1])?;
            // C-161 first: negatives clamp, the count ceiling aborts before
            // the offset is judged
            let (r, c) = norm_dims(want_int(name, &args[2])?, want_int(name, &args[3])?)?;
            let w: usize = match name {
                "matrix.from_bytes_f64_le" => 8,
                "matrix.from_bytes_f32_le" => 4,
                _ => 2,
            };
            // C-341: `Option[Matrix]` — `some` exactly when 0 <= off <= len
            // and r*c*width <= len - off (judged on the buffer side, never as
            // a wrapping `off + need`); `none` otherwise
            let len = b.len() as i128;
            let need = r as i128 * c as i128 * w as i128;
            if off < 0 || off as i128 > len || need > len - off as i128 {
                return Ok(Value::None);
            }
            let base = off as usize;
            let data: Vec<f64> = (0..(r * c) as usize)
                .map(|i| {
                    let p = base + i * w;
                    // a DECODER, not a dequantizer: the stored sign survives
                    match w {
                        8 => f64::from_le_bytes([
                            b[p],
                            b[p + 1],
                            b[p + 2],
                            b[p + 3],
                            b[p + 4],
                            b[p + 5],
                            b[p + 6],
                            b[p + 7],
                        ]),
                        4 => {
                            f32::from_bits(u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]))
                                as f64
                        }
                        _ => f16_to_f64(u16::from_le_bytes([b[p], b[p + 1]])),
                    }
                })
                .collect();
            Ok(Value::Some(Rc::new(mat(r, c, data))))
        }
        "matrix.zeros_f32" | "matrix.ones_f32" => {
            let base = if name == "matrix.ones_f32" {
                "matrix.ones"
            } else {
                "matrix.zeros"
            };
            dispatch(it, base, args)
        }
        "matrix.fma" => {
            arity(name, &args, 4)?;
            let sa = dispatch(it, "matrix.scale", vec![args[0].clone(), args[1].clone()])?;
            let sb = dispatch(it, "matrix.scale", vec![args[2].clone(), args[3].clone()])?;
            dispatch(it, "matrix.add", vec![sa, sb])
        }
        "matrix.fma3" => {
            arity(name, &args, 6)?;
            let ab = dispatch(it, "matrix.fma", args[0..4].to_vec())?;
            let sc = dispatch(it, "matrix.scale", vec![args[4].clone(), args[5].clone()])?;
            dispatch(it, "matrix.add", vec![ab, sc])
        }
        "matrix.mul_f32" => {
            arity(name, &args, 2)?;
            dispatch(it, "matrix.mul", args)
        }
        "matrix.mul_f32_t" => {
            arity(name, &args, 2)?;
            let t = dispatch(it, "matrix.transpose", vec![args[1].clone()])?;
            dispatch(it, "matrix.mul", vec![args[0].clone(), t])
        }
        "matrix.mul_scaled" | "matrix.mul_f32_scaled" | "matrix.mul_f32_t_scaled" => {
            arity(name, &args, 3)?;
            let b = if name == "matrix.mul_f32_t_scaled" {
                dispatch(it, "matrix.transpose", vec![args[2].clone()])?
            } else {
                args[2].clone()
            };
            let p = dispatch(it, "matrix.mul", vec![args[0].clone(), b])?;
            dispatch(it, "matrix.scale", vec![p, args[1].clone()])
        }
        // An operand with no rows yields the other; else base's rows then
        // extra's, and two non-empty operands must be the same width (C-353).
        "matrix.append_rows" => {
            arity(name, &args, 2)?;
            let base = want_mat(name, &args[0])?;
            let extra = want_mat(name, &args[1])?;
            if base.rows == 0 {
                return Ok(args[1].clone());
            }
            if extra.rows == 0 {
                return Ok(args[0].clone());
            }
            shape_eq(base.cols, extra.cols)?;
            let (r, c) = norm_dims(base.rows + extra.rows, base.cols)?;
            let mut data = base.data.clone();
            data.extend_from_slice(&extra.data);
            Ok(mat(r, c, data))
        }
        // The sequential widen-mul-add dot over the f32 weight rows read out of
        // the byte buffer. An empty x or a zero out/in extent is
        // `zeros(rows(x), w_rows)` (the C-161 ceiling included); a negative
        // offset reads from 0; weights that do not fit the buffer decode as
        // the all-zero rows.
        "matrix.linear_f32_row_no_bias" => {
            arity(name, &args, 5)?;
            let x = want_mat(name, &args[0])?;
            let b = want_bytes(name, &args[1])?;
            let off = want_int(name, &args[2])?.max(0);
            let w_rows = want_int(name, &args[3])?;
            let w_cols = want_int(name, &args[4])?;
            if x.rows == 0 || w_rows <= 0 || w_cols <= 0 {
                return dispatch(
                    it,
                    "matrix.zeros",
                    vec![Value::Int(x.rows), Value::Int(w_rows)],
                );
            }
            let (r, c) = norm_dims(w_rows, w_cols)?;
            let len = b.len() as i128;
            let need = r as i128 * c as i128 * 4;
            let fits = off as i128 <= len && need <= len - off as i128;
            let data: Vec<f64> = (0..(r * c) as usize)
                .map(|i| {
                    if !fits {
                        return 0.0;
                    }
                    let p = off as usize + i * 4;
                    f32::from_bits(u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])) as f64
                })
                .collect();
            dispatch(
                it,
                "matrix.linear_row_no_bias",
                vec![args[0].clone(), mat(r, c, data)],
            )
        }
        "matrix.to_bytes_f32_le" => {
            arity(name, &args, 1)?;
            let m = want_mat(name, &args[0])?;
            let mut out = Vec::with_capacity(m.data.len() * 4);
            for x in m.data.iter() {
                out.extend_from_slice(&(*x as f32).to_bits().to_le_bytes());
            }
            Ok(Value::Bytes(Rc::new(std::cell::RefCell::new(out))))
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
