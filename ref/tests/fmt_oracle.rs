//! Cross-check of the evaluator's OWN float formatter (fmtfloat: exact
//! big-integer Dragon4) against the host's shortest-round-trip formatter.
//! The host appears ONLY here, as a test oracle — the production path never
//! touches it (ADR-0015 clause 5). A disagreement is a bug in fmtfloat, since
//! both claim the same spec (ALS-T13: shortest round-tripping decimal).
use std::process::Command;

#[test]
fn display_matches_host_shortest() {
    // the bin crate exposes no lib; drive it as a process over a scratch file
    let exe = env!("CARGO_BIN_EXE_als-ref");
    let dir = std::env::temp_dir().join(format!("alsref-fmt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // deterministic pseudo-random bit patterns + adversarial edges
    let mut samples: Vec<u64> = vec![
        0x3FD3333333333334, // 0.1 + 0.2
        0x3FD3333333333333, // 0.3
        (0.1f64).to_bits(),
        (1.5f64).to_bits(),
        (10.0f64).to_bits(),
        (1e300f64).to_bits(),
        (5e-324f64).to_bits(),
        (2.2250738585072014e-308f64).to_bits(),
        (9007199254740993.0f64).to_bits(),
        (123456789.123456789f64).to_bits(),
        (1e78f64).to_bits(),
        (0.30000000000000004f64).to_bits(),
        (1669760939663944.25f64).to_bits(), // an exact tie: rounds UP
    ];
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..3000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        samples.push(x);
    }
    let mut prog = String::new();
    let mut expected = Vec::new();
    let mut idx = 0;
    prog.push_str("fn main() -> Unit = {\n");
    for &bits in &samples {
        let v = f64::from_bits(bits);
        if !v.is_finite() {
            continue;
        }
        // host oracle: Rust Display is positional shortest round-trip
        expected.push(format!("{v}"));
        prog.push_str(&format!(
            "  println(\"${{float.to_string(float.bits_to_float({}))}}\")\n",
            bits as i64
        ));
        idx += 1;
        let _ = idx;
    }
    prog.push_str("}\n");
    let path = dir.join("probe.almd");
    std::fs::write(&path, prog).unwrap();
    let out = Command::new(exe)
        .args(["run", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let doc: String = String::from_utf8_lossy(&out.stdout).to_string();
    let parsed: serde_lite::Doc = serde_lite::parse(&doc);
    let stdout = parsed.stdout;
    let got: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        got.len(),
        expected.len(),
        "line count; raw: {}",
        &doc[..doc.len().min(300)]
    );
    let mut bad = 0;
    for (i, (g, e)) in got.iter().zip(expected.iter()).enumerate() {
        // to_string keeps ".0" on integral values; the host drops it — normalize
        let g_norm = g.strip_suffix(".0").unwrap_or(g);
        let e_norm = e.strip_suffix(".0").unwrap_or(e);
        if g_norm != e_norm {
            bad += 1;
            if bad <= 10 {
                eprintln!(
                    "MISMATCH #{i}: mine={g:?} host={e:?} bits={:#x}",
                    f64::from_bits(samples[i].to_owned()).to_bits()
                );
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        bad,
        0,
        "{bad} of {} disagree with the host shortest formatter",
        expected.len()
    );
}

/// C-372: a Float32 in interpolation prints the host's f32 Display — the
/// shortest digits that round-trip to the same binary32 — over every biased
/// exponent (with the significand edges, both signs: every power of two, the
/// subnormal boundary, MIN/MAX, ±0, ±inf, NaN) and 3000 pseudo-random
/// bit patterns.
#[test]
fn f32_display_matches_host_shortest() {
    let exe = env!("CARGO_BIN_EXE_als-ref");
    let dir = std::env::temp_dir().join(format!("alsref-fmt32-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut samples: Vec<u32> = Vec::new();
    for e in 0..256u32 {
        for m in [0u32, 1, 2, 3, 1 << 22, (1 << 23) - 2, (1 << 23) - 1] {
            samples.push((e << 23) | m);
            samples.push((1 << 31) | (e << 23) | m);
        }
    }
    let mut x: u32 = 0x9E37_79B9;
    for _ in 0..3000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        samples.push(x);
    }
    let mut prog = String::from("fn show(b: Int) -> Unit = {\n  let g = float.to_float32(int.bits_to_float(b))\n  println(\"${g}\")\n}\n\nfn main() -> Unit = {\n");
    let mut expected = Vec::new();
    for &bits in &samples {
        let g = f32::from_bits(bits);
        expected.push(format!("{g}"));
        // the f64 carrying the same value (exact: every binary32 is a binary64)
        prog.push_str(&format!("  show({})\n", (g as f64).to_bits() as i64));
    }
    prog.push_str("}\n");
    let path = dir.join("probe32.almd");
    std::fs::write(&path, prog).unwrap();
    let out = Command::new(exe)
        .args(["run", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let doc = String::from_utf8_lossy(&out.stdout).to_string();
    let stdout = serde_lite::parse(&doc).stdout;
    let got: Vec<&str> = stdout.lines().collect();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        got.len(),
        expected.len(),
        "line count; raw: {}",
        &doc[..doc.len().min(300)]
    );
    let bad: Vec<String> = got
        .iter()
        .zip(expected.iter())
        .zip(samples.iter())
        .filter(|((g, e), _)| g != e)
        .map(|((g, e), b)| format!("bits={b:#010x} mine={g:?} host={e:?}"))
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} disagree:\n{}",
        bad.len(),
        expected.len(),
        bad[..bad.len().min(10)].join("\n")
    );
}

/// just enough JSON reading for the protocol reply (no external crates)
mod serde_lite {
    pub struct Doc {
        pub stdout: String,
    }
    pub fn parse(s: &str) -> Doc {
        // {"exit": n, "stdout": "…", "stderr": "…"}
        let key = "\"stdout\": \"";
        let start = s.find(key).map(|i| i + key.len()).unwrap_or(0);
        let bytes: Vec<char> = s[start..].chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                '"' => break,
                '\\' => {
                    i += 1;
                    match bytes.get(i) {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        Some('"') => out.push('"'),
                        Some('\\') => out.push('\\'),
                        Some('u') => {
                            let hex: String = bytes[i + 2..i + 6].iter().collect();
                            if let Ok(v) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(v) {
                                    out.push(c);
                                }
                            }
                            i += 5;
                        }
                        Some(&c) => out.push(c),
                        None => {}
                    }
                }
                c => out.push(c),
            }
            i += 1;
        }
        Doc { stdout: out }
    }
}

#[test]
fn parse_matches_host() {
    // fmtfloat::parse_decimal vs the host's FromStr, via float.parse through
    // the binary (the host appears only as the oracle)
    let exe = env!("CARGO_BIN_EXE_als-ref");
    let dir = std::env::temp_dir().join(format!("alsref-parse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut inputs: Vec<String> = vec![
        "0.30000000000000004",
        "2.2250738585072014e-308",
        "5e-324",
        "4.9e-324",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "9007199254740993",
        "0.1",
        "123456789012345678901234567890",
        "1e-400",
        "1e400",
        "0.000000000000000000000000000000000000000000001",
        "3.141592653589793238462643383279",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let mut x: u64 = 0x853C49E6748FEA9B;
    for _ in 0..1500 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let v = f64::from_bits(x);
        if v.is_finite() {
            inputs.push(format!("{v}"));
            inputs.push(format!("{:e}", v));
        }
    }
    let mut prog = String::from("import io\nfn main() -> Unit = {\n");
    let mut expected: Vec<u64> = Vec::new();
    for s in &inputs {
        let host: f64 = s.parse().unwrap();
        expected.push(if host.is_nan() {
            0x7FF8000000000000
        } else {
            host.to_bits()
        });
        prog.push_str(&format!(
            "  println(\"${{float.to_bits(float.parse('{s}') ?? -1.0)}}\")\n"
        ));
    }
    prog.push_str("}\n");
    let path = dir.join("probe.almd");
    std::fs::write(&path, prog).unwrap();
    let out = Command::new(exe)
        .args(["run", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let doc = String::from_utf8_lossy(&out.stdout).to_string();
    let parsed = serde_lite::parse(&doc);
    let got: Vec<&str> = parsed.stdout.lines().collect();
    assert_eq!(
        got.len(),
        expected.len(),
        "raw head: {}",
        &doc[..doc.len().min(300)]
    );
    let mut bad = 0;
    for (i, (g, e)) in got.iter().zip(expected.iter()).enumerate() {
        let mine: i64 = g.parse().unwrap_or(0);
        if mine as u64 != *e {
            bad += 1;
            if bad <= 10 {
                eprintln!(
                    "PARSE MISMATCH {:?}: mine bits {} host bits {}",
                    inputs[i], mine as u64, e
                );
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(bad, 0, "{bad}/{} parse disagreements", expected.len());
}
