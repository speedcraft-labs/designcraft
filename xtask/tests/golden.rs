#[path = "../src/task_policy.rs"]
mod task_policy;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn task(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask")).current_dir(root()).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    output
}

fn stdout(args: &[&str]) -> String {
    String::from_utf8(task(args).stdout).unwrap()
}

fn temporary(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("designcraft-xtask-70-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn version_matches_original_and_rewrites_temp_copy() {
    assert_eq!(stdout(&["version"]), include_str!("golden/version.stdout"));
    let temp = temporary("version");
    let manifest = temp.join("Cargo.toml");
    std::fs::write(&manifest, include_str!("golden/version.input.toml")).unwrap();
    let mut config = task_policy::configs(temp.clone()).version;
    config.refresh_lockfile = false;
    config.manifest_paths = vec!["Cargo.toml".into()];
    assert_eq!(craft_xtask::version::read(&std::fs::read_to_string(&manifest).unwrap()).unwrap(), "0.1.0");
    assert_eq!(
        format!("version: {} -> 0.2.0-rc.1\n", craft_xtask::version::read(include_str!("golden/version.input.toml")).unwrap()),
        include_str!("golden/version-set.stdout")
    );
    assert_eq!(
        craft_xtask::version::rewrite(&config, include_str!("golden/version.input.toml"), "0.2.0-rc.1").unwrap(),
        include_str!("golden/version.updated.toml")
    );
    craft_xtask::version::run(&config, &["set".into(), "0.2.0-rc.1".into()]).unwrap();
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), include_str!("golden/version.updated.toml"));
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn stats_matches_original_except_changed_xtask_source() {
    let original = include_str!("golden/stats.stdout");
    let actual = stdout(&["stats"]);
    let stable =
        |text: &str| text.lines().filter(|line| !line.starts_with("xtask ") && !line.starts_with("TOTAL ")).map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(stable(&actual), stable(original));
    assert_eq!(craft_xtask::stats::count_tests("#[test]\nfn a() {}\n// #[test]\nproptest! {\n}"), (1, 1));
}

#[test]
fn layers_matches_original_report_and_policy() {
    assert_eq!(stdout(&["layers"]), include_str!("golden/layers.stdout"));
    let config = task_policy::configs(root()).layers;
    assert_eq!(task_policy::short_name("designcraft-geom"), "geom");
    assert_eq!(task_policy::describe(&config, "designcraft-geom"), "L0");
    use craft_xtask::layers::{Crate, Dep, DepKind::*};
    let edge = |from: &str, to: &str, kind, workspace| {
        craft_xtask::layers::check(&config, &[Crate { name: from.into(), deps: vec![Dep { name: to.into(), kind, workspace }] }]).unwrap()
    };
    assert!(edge("designcraft-doc", "designcraft-geom", Normal, true).is_empty());
    assert!(edge("designcraft-color", "designcraft-geom", Normal, true).is_empty());
    assert!(edge("designcraft-fonts", "designcraft-doc", Normal, true).is_empty());
    assert!(!edge("designcraft-geom", "designcraft-color", Normal, true).is_empty());
    assert!(!edge("designcraft-geom", "designcraft-engine", Normal, true).is_empty());
    assert!(!edge("designcraft-compose", "designcraft-images", Normal, true).is_empty());
    assert!(edge("designcraft-render", "designcraft-testkit", Dev, true).is_empty());
    assert!(!edge("designcraft-render", "designcraft-testkit", Normal, true).is_empty());
    assert!(!edge("designcraft-engine", "egui", Normal, false).is_empty());
    assert!(edge("designcraft-ui-egui", "egui", Normal, false).is_empty());
    assert!(edge("designcraft", "designcraft-ui-egui", Normal, true).is_empty());
}

#[test]
fn pdf_deps_matches_original_report_and_pins() {
    assert_eq!(stdout(&["pdf-deps"]), include_str!("golden/pdf-deps.stdout"));
    let mut config = task_policy::configs(root()).pdf_deps;
    let metadata = config.cargo.metadata(true).unwrap();
    assert!(craft_xtask::pdf_deps::validate(&config, &metadata).is_ok());
    config.packages[0].version = "9.9.9".into();
    assert!(craft_xtask::pdf_deps::validate(&config, &metadata).is_err());
}

#[test]
fn parity_matches_original_and_preserves_checklist() {
    let checklist = root().join("docs/parity-checklist.md");
    let before = std::fs::read(&checklist).unwrap();
    assert_eq!(stdout(&["parity"]), include_str!("golden/parity.stdout"));
    assert_eq!(std::fs::read(&checklist).unwrap(), before);
}

#[test]
fn ico_matches_original_bytes() {
    let temp = temporary("ico");
    let out = temp.join("test.ico");
    let output = stdout(&[
        "ico",
        out.to_str().unwrap(),
        "assets/app-icon/hicolor/16x16/apps/ai.storyteller.designcraft.png",
        "assets/app-icon/hicolor/32x32/apps/ai.storyteller.designcraft.png",
    ]);
    assert_eq!(output.replace(out.to_str().unwrap(), "/tmp/designcraft70.ico"), include_str!("golden/ico.stdout"));
    let hex = include_str!("golden/ico.hex").trim();
    let expected: Vec<u8> =
        hex.as_bytes().as_chunks::<2>().0.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect();
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn help_includes_shared_task_syntax() {
    let text = stdout(&["--help"]);
    for expected in [
        "assets",
        "layers",
        "pdf-deps",
        "parity",
        craft_xtask::version::cli_help(),
        craft_xtask::stats::cli_help(),
        craft_xtask::layers::cli_help(),
        craft_xtask::parity::cli_help(),
        craft_xtask::pdf_deps::cli_help(),
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
}
