use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("read rcheevos sources")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "c") {
            out.push(path);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    sources(Path::new("rcheevos/src"), &mut files);
    files.sort();
    cc::Build::new()
        .files(&files)
        .file("shim/romp_cheevos.c")
        .include("rcheevos/include")
        .include("rcheevos/src")
        .include("libretro")
        .define("RC_CLIENT_SUPPORTS_HASH", None)
        .warnings(false)
        .compile("rcheevos");
    println!("cargo:rerun-if-changed=rcheevos");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=libretro");
}
