//! Workspace tooling: `cargo xtask <command>`.
//!
//! Shared native tasks plus app-specific commands. External tools (`cargo`, `curl`, `tar`) are
//! invoked through `std::process::Command`.

mod assets;
mod task_policy;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets          check that every icon/image/font/asset is attributed in ASSETS.md
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  pdf-deps        verify the pinned PDF backend, including Krilla dependencies
  wasm            cargo check --target wasm32-unknown-unknown for the wasm-safe crates (one call)
  test            workspace tests (cargo nextest when installed, else cargo test), then doctests
  i18n-report     list the interface strings each language shows in English (always passes)
  ci              fmt --check, pdf-deps, clippy -D warnings, test, i18n-report, doctest, assets,
                  layers, wasm (stops at first failure)
  corpus [--download]
                  show where test corpora live; --download fetches PngSuite into corpus/pngsuite
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  parity          recompute the InDesign feature-parity summary in docs/parity-checklist.md
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let shared_args: Vec<String> = args.iter().skip(1).cloned().collect();
    let configs = task_policy::configs(root());
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(&configs.layers, &shared_args),
        Some("pdf-deps") => cmd_pdf_deps(&configs.pdf_deps, &shared_args),
        Some("wasm") => cmd_wasm(),
        Some("test") => cmd_test().and_then(|()| cmd_doctest()),
        Some("i18n-report") => cmd_i18n_report(),
        Some("ci") => cmd_ci(),
        Some("corpus") => cmd_corpus(rest.contains(&"--download")),
        Some("stats") => craft_xtask::stats::run(&configs.stats, &shared_args),
        Some("parity") => cmd_parity(&configs.parity, &shared_args),
        Some("ico") => craft_xtask::ico::run(&configs.ico, &shared_args),
        Some("version") => craft_xtask::version::run(&configs.version, &shared_args),
        Some("-h" | "--help" | "help") | None => {
            print!("{}", usage());
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{}", usage())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    format!(
        "{USAGE}\nShared task syntax:\n  {}\n  {}\n  {}\n  {}\n  {}\n  {}\n",
        craft_xtask::version::cli_help(),
        craft_xtask::stats::cli_help(),
        craft_xtask::ico::cli_help().replace("ico/icns <out.ico|out.icns>", "ico <out.ico>"),
        craft_xtask::layers::cli_help(),
        craft_xtask::parity::cli_help(),
        craft_xtask::pdf_deps::cli_help()
    )
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers(config: &craft_xtask::layers::Config, args: &[String]) -> Result<(), String> {
    let report = craft_xtask::layers::compute(config, &craft_xtask::layers::Args::parse(args)?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &report.crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    craft_xtask::layers::DepKind::Normal => "",
                    craft_xtask::layers::DepKind::Dev => " (dev)",
                    craft_xtask::layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", task_policy::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, task_policy::describe(config, &c.name), ws.join(", "));
    }
    println!();
    if report.ok {
        println!("OK: {} crates, no layering violations.", report.crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", report.violations.len());
        for v in &report.violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", report.violations.len()))
    }
}

fn cmd_pdf_deps(config: &craft_xtask::pdf_deps::Config, args: &[String]) -> Result<(), String> {
    let _report = craft_xtask::pdf_deps::compute(config, &craft_xtask::pdf_deps::Args::parse(args)?)?;
    println!("PDF backend: facade, Krilla and Hayro resolve once to {}", task_policy::PDF_REV);
    Ok(())
}

fn cmd_parity(config: &craft_xtask::parity::Config, args: &[String]) -> Result<(), String> {
    let args = craft_xtask::parity::Args::parse(args)?;
    let report = craft_xtask::parity::compute(config, &args)?;
    if !args.check {
        craft_xtask::parity::apply(&report)?;
    }
    print!("{}", craft_xtask::parity::summary(&report.rows));
    Ok(())
}

/// Workspace packages that must build for wasm32: all L0–L5 crates plus
/// the egui shell.
fn wasm_set() -> Result<Vec<String>, String> {
    let config = task_policy::configs(root()).layers;
    let crates = craft_xtask::layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|krate| {
            matches!(task_policy::describe(&config, &krate.name).as_str(), "L0" | "L1" | "L2" | "L3" | "L4" | "L5")
                || task_policy::short_name(&krate.name) == "ui-egui"
        })
        .map(|c| c.name)
        .collect())
}

/// One `cargo check` for the whole set: each crate is checked once, and
/// `--keep-going` still reports every failing crate. Features unify across the
/// set, so each crate is checked with the union of the features the set enables.
fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    if set.is_empty() {
        return Err("no wasm-safe crates found in the workspace metadata".into());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for pkg in &set {
        c.args(["-p", pkg]);
    }
    run(c, &format!("cargo check --target wasm32-unknown-unknown --keep-going -p {}", set.join(" -p ")))?;
    println!("\nwasm32-unknown-unknown check: ok ({} crates: {})", set.len(), set.join(", "));
    Ok(())
}

/// Test targets for the test build. Examples are left out so they aren't
/// linked; clippy `--all-targets` still compiles them.
const TEST_TARGETS: [&str; 4] = ["--workspace", "--lib", "--bins", "--tests"];

fn nextest_available() -> bool {
    cargo().args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn cmd_test() -> Result<(), String> {
    let mut c = cargo();
    let tool = if nextest_available() {
        c.args(["nextest", "run"]);
        "nextest run"
    } else {
        eprintln!("note: cargo-nextest is not installed, running cargo test (cargo install cargo-nextest --locked)");
        c.arg("test");
        "test"
    };
    c.args(TEST_TARGETS);
    run(c, &format!("cargo {tool} {}", TEST_TARGETS.join(" ")))
}

/// Runs the ignored `missing_translations_report` test with its output shown. It takes the test
/// step's targets, so it reuses that build.
fn cmd_i18n_report() -> Result<(), String> {
    const REPORT: &str = "i18n::tests::missing_translations_report";
    let mut c = cargo();
    if nextest_available() {
        c.args(["nextest", "run", "--cargo-quiet"]).args(TEST_TARGETS);
        c.args(["--run-ignored", "only", "--no-capture", "--status-level", "fail", "--final-status-level", "fail", "-E"]);
        c.arg(format!("package(designcraft-ui-egui) & test(={REPORT})"));
    } else {
        c.args(["test", "-q"]).args(TEST_TARGETS).args(["--", "--ignored", "--exact", REPORT, "--nocapture"]);
    }
    run(c, &format!("missing translations ({REPORT})"))
}

/// nextest does not run doctests.
fn cmd_doctest() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        ("pdf-deps", Box::new(|| cmd_pdf_deps(&task_policy::configs(root()).pdf_deps, &[]))),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("i18n-report", Box::new(cmd_i18n_report)),
        ("doctest", Box::new(cmd_doctest)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(|| cmd_layers(&task_policy::configs(root()).layers, &[]))),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}

const PNGSUITE_URL: &str = "http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz";

fn cmd_corpus(download: bool) -> Result<(), String> {
    let corpus = root().join("corpus");
    println!(
        "Test corpora live under {} (git-ignored, never committed).
Tests that use a corpus skip cleanly when it is absent.

  corpus/pngsuite/   PngSuite (public domain) — designcraft-codecs compares every file
                     against the `image` crate. Fetch: cargo xtask corpus --download
  corpus/psd/        PSD samples from MIT/BSD projects (ag-psd, psd-tools test data).
                     Copy files in manually; licences must be MIT/BSD/CC0.
  corpus/tiff/       libtiff pics (optional)
  corpus/exr/        OpenEXR sample images (optional)
  corpus/raw/        raw.pixls.us samples, CC0 (optional)
",
        corpus.display()
    );
    if !download {
        return Ok(());
    }
    let dest = corpus.join("pngsuite");
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let tgz = corpus.join("PngSuite-2017jul19.tgz");
    let mut curl = Command::new("curl");
    curl.args(["-fsSL", "-o"]).arg(&tgz).arg(PNGSUITE_URL);
    run(curl, &format!("curl {PNGSUITE_URL}"))?;
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tgz).arg("-C").arg(&dest);
    run(tar, "tar -xzf PngSuite-2017jul19.tgz")?;
    let _ = std::fs::remove_file(&tgz);
    let n = std::fs::read_dir(&dest).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "png")).count()).unwrap_or(0);
    println!("PngSuite: {n} PNG files in {}", dest.display());
    if n == 0 { Err("no PNG files extracted".into()) } else { Ok(()) }
}
