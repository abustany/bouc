use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=assets/styles.css");
    println!("cargo:rerun-if-changed=assets/day-popover.js");

    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is not set");
    let js_build_path = format!("{out_dir}/day-popover.js");

    let mut tailwind_args = vec!["--input", "src/styles.css", "--output", "assets/styles.css"];
    let mut rolldown_args = vec![
        "src/day-popover.ts",
        "--file",
        js_build_path.as_str(),
        "--format",
        "iife",
        "--platform",
        "browser",
    ];

    if release {
        tailwind_args.push("--minify");
        rolldown_args.push("--minify");
    }

    run("tailwindcss", &tailwind_args);
    run("rolldown", &rolldown_args);

    // rolldown rewrites its output on every run, which would make cargo see
    // assets/day-popover.js as dirty and rerun this script every build
    copy_if_changed(&js_build_path, "assets/day-popover.js");
}

fn copy_if_changed(from: &str, to: &str) {
    let contents = std::fs::read(from).unwrap_or_else(|e| panic!("failed to read {from}: {e}"));

    if std::fs::read(to).is_ok_and(|existing| existing == contents) {
        return;
    }

    std::fs::write(to, contents).unwrap_or_else(|e| panic!("failed to write {to}: {e}"));
}

fn run(program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program}: {e}"));

    assert!(status.success(), "{program} exited with {status}");
}
