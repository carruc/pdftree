fn main() {
    println!("cargo:rerun-if-changed=src/poppler.c");
    let lib = pkg_config::probe_library("poppler-glib")
        .expect("install Poppler GLib development headers");
    let cairo = pkg_config::probe_library("cairo").expect("install Cairo development headers");
    cc::Build::new()
        .file("src/poppler.c")
        .includes(lib.include_paths)
        .includes(cairo.include_paths)
        .compile("pdf_tree_poppler");
}
