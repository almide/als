//! Minimal regression programs for evaluator faults found on corpus fixtures.
//! Each case is the smallest program that reproduced the fault, with the
//! stdout almide prints for it on native and wasm. A fault (`{"error": …}`)
//! or an abstain here is a failure: the case was fixed, so it must evaluate.
use std::process::Command;

fn run(tag: &str, src: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_als-ref");
    let dir = std::env::temp_dir().join(format!("alsref-reg-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("case.almd");
    std::fs::write(&file, src).unwrap();
    let out = Command::new(exe)
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{tag}: als-ref did not run the case to exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );
    stdout
}

/// declaration_literal_domain: an Int assigned into a sized `var` (here a
/// top-level one, whose upper-case name lexes as a type name) keeps the
/// slot's width, so `uint8.to_string` reads a UInt8, not an Int.
#[test]
fn assignment_into_a_sized_slot_keeps_its_width() {
    let src = r#"
var TOP_VAR: UInt8 = 255
var TOP_LIST_VAR: List[UInt8] = [1, 2]

type Edges = { n: Int, u8: UInt8 = 255 }

fn main() -> Unit = {
  TOP_VAR = 254
  TOP_LIST_VAR[1] = 255
  var cell = Edges { n: 2 }
  cell.u8 = 7
  var slot: UInt8 = 1
  slot = 9
  let e = Edges { n: 1 }
  println("${uint8.to_string(TOP_VAR)} ${TOP_LIST_VAR} ${uint8.to_string(cell.u8)} ${uint8.to_string(slot)} ${uint8.to_string(e.u8)}")
}
"#;
    assert_eq!(run("sized-assign", src), "254 [1, 255] 7 9 255\n");
}

/// fallible_hof_matrix_*: a fallible callback puts the call in first-err
/// form even on the path that never runs it — an out-of-range
/// `list.update`, a `map.upsert` that inserts, an `option.or_else` /
/// `option.unwrap_or_else` on `some`. Before, those paths answered the bare
/// value and the `match r { ok(..) / err(..) }` reading it had no arm.
#[test]
fn a_fallible_callback_that_never_runs_still_gives_the_carrier() {
    let src = r#"
effect fn add(a: Int, x: Int) -> Int = {
  if x < 0 then err("bad add")!
  else a + x
}

effect fn dbl(v: Int) -> Int = {
  if v < 0 then err("bad dbl")!
  else v * 2
}

effect fn seven() -> Int? = some(7)

effect fn nothing() -> Int = err("no fallback")!

fn show[T](label: String, r: Result[T, String], f: (T) -> String) -> Unit = match r {
  ok(v) => println(label + " ok " + f(v)),
  err(e) => println(label + " err " + e),
}

fn oi(o: Int?) -> String = match o {
  some(n) => "some " + int.to_string(n),
  none => "none",
}

fn ints(xs: List[Int]) -> String = xs |> list.map((n) => int.to_string(n)) |> list.join(",")

fn mi(m: Map[String, Int]) -> String = map.to_list(m) |> list.map((e) => e.0 + "=" + int.to_string(e.1)) |> list.join(",")

effect fn main() -> Unit = {
  let xs = [1, 3, 5]
  show("update", list.update(xs, 1, (x) => add(x, 100)!), ints)
  show("update", list.update(xs, 7, (x) => add(x, -1)!), ints)
  show("update", list.update(xs, 1, (x) => add(x, -1)!), ints)
  let m = map.from_list([("a", 1), ("c", 5)])
  show("upsert", map.upsert(m, "c", 0, (v) => dbl(v)!), mi)
  show("upsert", map.upsert(m, "z", 9, (v) => dbl(v)!), mi)
  show("or_else", some(2) |> option.or_else(() => seven()!), oi)
  show("or_else", none |> option.or_else(() => seven()!), oi)
  show("unwrap_or_else", some(1) |> option.unwrap_or_else(() => nothing()!), (n) => int.to_string(n))
  show("unwrap_or_else", none |> option.unwrap_or_else(() => nothing()!), (n) => int.to_string(n))
}
"#;
    assert_eq!(
        run("never-run-carrier", src),
        "update ok 1,103,5\nupdate ok 1,3,5\nupdate err bad add\n\
         upsert ok a=1,c=10\nupsert ok a=1,c=5,z=9\n\
         or_else ok some 2\nor_else ok some 7\n\
         unwrap_or_else ok 1\nunwrap_or_else err no fallback\n"
    );
}

/// fallible_hof_recursive_callee / list_find_map_short_circuit: in
/// `xs |> list.map(f)!` the `!` binds to the pipe stage, so the stage is the
/// call `list.map(xs, f)` and the `!` reads its result. Before, the stage was
/// called with the callback alone.
#[test]
fn a_bang_on_a_pipe_stage_reads_the_piped_call() {
    let src = r#"
effect fn big(s: String) -> Int? = {
  let n = int.parse(s)!
  if n > 10 then some(n) else none
}

effect fn twice(x: Int) -> Int = if x < 0 then err("neg")! else x * 2

effect fn total(xs: List[Int]) -> Int = {
  let ys = xs |> list.map((x) => twice(x)!)!
  list.sum(ys)
}

effect fn main() -> Unit = {
  let hit = ["1", "20", "30"] |> list.find_map((s) => big(s)!)!
  println("hit ${hit ?? -1}")
  let t = total([1, 2, 3])!
  println("total ${t}")
}
"#;
    assert_eq!(run("pipe-stage-bang", src), "hit 20\ntotal 12\n");
}
