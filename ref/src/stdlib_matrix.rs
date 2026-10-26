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
    "matrix.from_bytes_f32_le",
    "matrix.from_bytes_f16_le",
    "matrix.from_bytes_f64_le",
    // C-353 / C-354 / C-355: the shape-precondition, row-range and
    // signed-zero rules, and the exact-arithmetic kernels they govern.
    "matrix.cols",
    "matrix.transpose",
    "matrix.scale",
    "matrix.map",
    "matrix.broadcast_add_row",
    "matrix.slice_rows",
    "matrix.gather_rows",
    "matrix.split_cols_even",
    "matrix.concat_cols",
    "matrix.concat_cols_many",
    "matrix.concat_rows",
    "matrix.to_bytes_f32_le",
    // the operator kernels (stdlib/matrix.almd): `+ - * /` and unary `-`
    // on Matrix desugar to these, and concat_rows folds __append_rows
    "matrix.__add",
    "matrix.__sub",
    "matrix.__div",
    "matrix.__mul",
    "matrix.__neg",
    "matrix.__append_rows",
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
        "matrix.__neg" => {
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
        "matrix.__add" | "matrix.__sub" | "matrix.__div" => {
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
                        "matrix.__add" => x + y,
                        "matrix.__sub" => x - y,
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
        "matrix.__mul" => {
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
        // gather_rows (almide_rt_matrix_gather_rows): a 0-row m answers 0 x 0;
        // otherwise one row per index, an index outside 0..rows (a negative
        // one included) the zero row of m's width
        "matrix.gather_rows" => {
            arity(name, &args, 2)?;
            let m = want_mat(name, &args[0])?;
            let ids = want_rids(name, &args[1])?;
            if m.rows == 0 {
                return Ok(mat(0, 0, Vec::new()));
            }
            let cols = m.cols as usize;
            let mut data = Vec::with_capacity(ids.len() * cols);
            for id in ids.iter() {
                if *id >= 0 && *id < m.rows {
                    let at = *id as usize * cols;
                    data.extend_from_slice(&m.data[at..at + cols]);
                } else {
                    data.extend(std::iter::repeat_n(0.0f64, cols));
                }
            }
            Ok(mat(ids.len() as i64, m.cols, data))
        }
        // concat_rows (stdlib/matrix.almd): __append_rows folded over the
        // list from the 0 x 0 matrix
        "matrix.concat_rows" => {
            arity(name, &args, 1)?;
            let xs = match &args[0] {
                Value::List(xs) => xs.clone(),
                other => {
                    return Err(Flow::Fatal(format!(
                        "{name}: expected List of Matrix, got {}",
                        other.type_name()
                    )))
                }
            };
            let mut acc = mat(0, 0, Vec::new());
            for m in xs.iter() {
                acc = dispatch(it, "matrix.__append_rows", vec![acc, m.clone()])?;
            }
            Ok(acc)
        }
        "matrix.slice_rows" => {
            arity(name, &args, 3)?;
            let m = want_mat(name, &args[0])?;
            let start = want_int(name, &args[1])?;
            let end = want_int(name, &args[2])?;
            let e = end.clamp(0, m.rows);
            let start = start.max(0);
            if start >= e {
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
        // An operand with no rows yields the other; else base's rows then
        // extra's, and two non-empty operands must be the same width (C-353).
        "matrix.__append_rows" => {
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
