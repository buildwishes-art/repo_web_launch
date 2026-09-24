fn main() {
    // tauri-build menanam icons/icon.ico ke resource .exe, tetapi tidak memantau folder ikon —
    // tanpa ini, mengganti ikon tidak ikut ter-embed (exe tetap memakai ikon lama).
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
