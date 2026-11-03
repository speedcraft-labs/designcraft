use std::path::PathBuf;

use craft_xtask::{Cargo, Configs, layers, pdf_deps};

pub const PDF_REV: &str = "92d392cff132eaf53e13afff77f38b5c11b73aa0";
const PDF_REPO: &str = "https://github.com/storytold/craft-libs";

pub fn short_name(name: &str) -> &str {
    name.strip_prefix("designcraft-").unwrap_or(name)
}

pub fn describe(config: &layers::Config, name: &str) -> String {
    if config.exempt.iter().any(|item| item == name) {
        return "exempt".into();
    }
    if config.testkits.iter().any(|item| item == name) {
        return "testkit".into();
    }
    config.layers.iter().find(|(_, names)| names.iter().any(|item| item == name)).map_or_else(|| "UNREGISTERED".into(), |(layer, _)| layer.clone())
}

pub fn configs(root: PathBuf) -> Configs {
    let cargo = Cargo { root: root.clone(), ..Cargo::default() };
    let mut configs = Configs::for_root(&root);
    configs.version.cargo = cargo.clone();
    configs.stats.cargo = cargo.clone();
    configs.stats.follow_symlinks = true;
    configs.ico.root = root.clone();
    configs.parity.cargo = cargo.clone();
    configs.layers = layers::Config {
        root: root.clone(),
        layers: [
            ("L0", &["geom", "color"][..]),
            ("L1", &["doc", "fonts"]),
            ("L2", &["compose", "images"]),
            ("L3", &["render", "pdf", "idml", "epub", "pptx", "textimport", "format"]),
            ("L4", &["tools"]),
            ("L5", &["engine"]),
            ("L6", &["ui-egui", "mcp", "testkit"]),
        ]
        .into_iter()
        .map(|(level, names)| (level.into(), names.iter().map(|name| format!("designcraft-{name}")).collect()))
        .collect(),
        exempt: ["designcraft", "designcraft-cli", "designcraft-web", "xtask"].into_iter().map(str::to_owned).collect(),
        testkits: vec!["designcraft-testkit".into()],
        allowed_edges: vec![("designcraft-color".into(), "designcraft-geom".into()), ("designcraft-fonts".into(), "designcraft-doc".into())],
        external_rules: vec![layers::ExternalRule {
            dependencies: ["craft-ui", "craft-window", "egui", "eframe", "winit", "egui_kittest", "rfd", "bevy*"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            allowed_from: ["designcraft-ui-egui", "designcraft-mcp", "designcraft-testkit"].into_iter().map(str::to_owned).collect(),
            allowed_dev_from: Vec::new(),
        }],
        ..layers::Config::default()
    };
    let source = pdf_deps::git_source(PDF_REPO, PDF_REV);
    configs.pdf_deps = pdf_deps::Config {
        cargo,
        packages: [
            ("craft-pdf", "0.1.0"),
            ("krilla", "0.8.2"),
            ("hayro", "0.7.1"),
            ("hayro-interpret", "0.7.0"),
            ("hayro-syntax", "0.7.2"),
            ("hayro-jbig2", "0.3.0"),
            ("hayro-cmap", "0.1.0"),
            ("hayro-postscript", "0.1.0"),
        ]
        .into_iter()
        .map(|(name, version)| pdf_deps::PackagePin {
            name: name.into(),
            version: version.into(),
            source: source.clone(),
            required_features: if name == "craft-pdf" {
                ["read", "interpret", "raster", "write", "write-pdf"].into_iter().map(str::to_owned).collect()
            } else {
                Vec::new()
            },
            exact_features: None,
        })
        .collect(),
        ..pdf_deps::Config::default()
    };
    configs
}
