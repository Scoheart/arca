use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest path"));
    let dist = manifest.join("../playground/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output path"))
        .join("playground_assets.rs");

    let mut assets = Vec::new();
    let index = dist.join("index.html");
    // Keep Cargo-only builds usable. Release preparation builds the frontend
    // first; attempting to open an unbuilt playground returns a clear error.
    if index.is_file() && dist.join("assets").is_dir() {
        assets.push(("/".to_owned(), "text/html; charset=utf-8", index));
        let mut entries: Vec<_> = fs::read_dir(dist.join("assets")).expect("Vite asset directory")
            .map(|entry| entry.expect("Vite asset entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if let Some(mime) = mime_for(&path).filter(|_| path.is_file()) {
                let file = path.file_name().and_then(|name| name.to_str()).expect("UTF-8 asset name");
                assets.push((format!("/assets/{file}"), mime, path));
            }
        }
    }
    let entries = assets.into_iter().map(|(url, mime, path)| {
        let path = path.canonicalize().expect("Vite asset path");
        format!("WebAsset {{ url: {url:?}, mime: {mime:?}, bytes: include_bytes!({:?}) }},\n",
            path.to_str().expect("UTF-8 asset path"))
    }).collect::<String>();
    fs::write(destination, format!("&[\n{entries}]\n")).expect("write playground asset manifest");
}

fn mime_for(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "js" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "woff2" => Some("font/woff2"),
        _ => None,
    }
}
