# DesignCraft xtask

The `xtask` binary dispatches `version`, `stats`, `ico`, `layers`, `parity`, and
`pdf-deps` to `craft-xtask`. DesignCraft's layer table and PDF revision pins live
in `src/task_policy.rs`. The original successful `layers`, `parity`, and `pdf-deps`
report text stays in the app because the shared `run(Config, args)` prints a
different report; their checking, graph parsing, and summary logic are shared.
`assets`, `wasm`, `test`, `i18n-report`, `ci`, and `corpus` remain app-specific.

`tests/golden/` captures the original command output on the pre-migration
checkout (2026-11-03). The ICO fixture is the original binary as hexadecimal;
the version manifest fixtures exercise a temporary copy without modifying the
workspace. Statistics necessarily change in the `xtask` row and totals when
the source modules are removed, so the test compares every other byte of the
original report.
