use capsules_lib::RUNTIME_TARGETS;
use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut entries: Vec<String> = Vec::new();

    println!("cargo:rerun-if-changed=../capsules_runtime");
    println!("cargo:rustc-include-src-dir={}", out_dir.to_string_lossy());

    for (target, extension) in RUNTIME_TARGETS {
        let path = manifest_dir
            .parent()
            .unwrap()
            .join("target")
            .join(target)
            .join("release")
            .join(format!("capsules_runtime{extension}"))
            .canonicalize()
            .unwrap();
        if path.exists() {
            entries.push(format!(
                "(\"{}\", include_bytes!(\"{}\"))",
                target,
                path.display()
            ));
        }
    }
    let generated = format!(
        "pub static RUNTIME_BINARIES: [(&str, &'static [u8]); {}] = [{}];",
        entries.len(),
        entries.join(",\n")
    );
    fs::write(out_dir.join("runtime_binaries.rs"), generated).unwrap();
}
