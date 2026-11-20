use std::path::Path;

fn main() {
    // `frontendDist` is embedded at compile time. A fresh checkout has no UI build yet, so give
    // it a placeholder page; `just run` and `just build` build the real UI first.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(&dist).expect("create ui/dist");
        std::fs::write(
            &index,
            "<!doctype html><meta charset=\"utf-8\"><title>Simple Capture</title>\
             <p>UI not built: run <code>just build</code>.</p>\n",
        )
        .expect("write placeholder ui/dist/index.html");
    }
    println!("cargo:rerun-if-changed=../ui/dist");
    tauri_build::build()
}
