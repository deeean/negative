use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn project_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent().unwrap()
        .parent().unwrap()
        .to_path_buf()
}

fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn run(cmd: &mut Command) {
    println!("[*] Running: {:?}", cmd);
    let status = cmd.status().expect("failed to execute command");
    if !status.success() {
        panic!("command exited with {}", status);
    }
}

fn build() {
    let root = project_root();
    let dist = root.join("dist");
    let _ = fs::remove_dir_all(&dist);
    fs::create_dir_all(&dist).unwrap();

    println!("[*] Building injector (tauri build)...");
    run(Command::new("bunx")
        .current_dir(root.join("crates").join("injector"))
        .args(["tauri", "build"]));

    println!("[*] Building 32-bit (payload + helper)...");
    run(Command::new(cargo())
        .current_dir(&root)
        .args(["build", "--release", "-p", "payload", "-p", "helper",
               "--target", "i686-pc-windows-msvc"]));

    println!("[*] Collecting artifacts...");
    let release = root.join("target").join("release");
    let release32 = root.join("target").join("i686-pc-windows-msvc").join("release");

    let copies: &[(&str, &str)] = &[
        ("injector.exe", "injector.exe"),
        ("payload.dll", "payload.dll"),
    ];
    for (src, dst) in copies {
        fs::copy(release.join(src), dist.join(dst))
            .unwrap_or_else(|e| panic!("failed to copy {src}: {e}"));
    }

    fs::copy(release32.join("payload.dll"), dist.join("payload32.dll"))
        .unwrap_or_else(|e| panic!("failed to copy payload32.dll: {e}"));
    fs::copy(release32.join("helper.exe"), dist.join("helper32.exe"))
        .unwrap_or_else(|e| panic!("failed to copy helper32.exe: {e}"));

    println!("[*] Done! Output in dist/");
    for entry in fs::read_dir(&dist).unwrap() {
        let entry = entry.unwrap();
        let meta = entry.metadata().unwrap();
        println!("  {} ({} bytes)", entry.file_name().to_string_lossy(), meta.len());
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let task = args.get(1).map(|s| s.as_str());

    match task {
        Some("build") | None => build(),
        Some(other) => {
            eprintln!("unknown task: {other}");
            eprintln!("usage: cargo xtask [build]");
            std::process::exit(1);
        }
    }
}
