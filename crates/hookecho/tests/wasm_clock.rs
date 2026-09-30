//! `std::time::Instant::now()` panics on wasm ("time not implemented on this platform"), and only
//! there: every native test passes over it. It took the web build down when Surface obs pulled
//! in the buoy cache. Code that runs in the browser takes its clock from `wxdata::clock`; this
//! keeps it that way by reading the source, since no native test can see the panic.

use std::path::Path;

/// Modules the web build does not compile (`#[cfg(not(target_arch = "wasm32"))]` at their `mod`),
/// where the std clock is fine.
const NATIVE_ONLY: &[&str] = &[
    "clock.rs",
    "headless.rs",
    "local_api.rs",
    "object_store.rs",
    "perf.rs",
    "plugins.rs",
    "serve.rs",
    "soak.rs",
];

fn scan(dir: &Path, bad: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, bad);
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".rs") || NATIVE_ONLY.contains(&name.as_str()) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        // Tests run natively only; stop at the test module.
        let body = src.split("#[cfg(test)]").next().unwrap_or("");
        let lines: Vec<&str> = body.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let std_instant = code.contains("std::time::Instant")
                || (code.contains("use std::time::{") && code.contains("Instant"));
            if !std_instant {
                continue;
            }
            // A native-only item right above it (a cfg'd fn, block or statement) is fine.
            let guarded = lines[i.saturating_sub(12)..i].iter().any(|l| {
                l.contains("cfg(not(target_arch = \"wasm32\"))")
                    || l.contains("cfg(target_os = \"android\")")
            });
            if !guarded {
                bad.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }
}

#[test]
fn browser_code_never_reads_the_std_clock() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut bad = Vec::new();
    scan(&root.join("src"), &mut bad);
    scan(&root.join("../wxdata/src"), &mut bad);
    assert!(
        bad.is_empty(),
        "std::time::Instant panics on wasm; use wxdata::clock::Instant:\n{}",
        bad.join("\n")
    );
}
