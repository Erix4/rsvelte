fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("static_files");
    let source_path = std::path::Path::new("src/static_files");

    if source_path.exists() && source_path.is_dir() {
        std::fs::create_dir_all(&dest_path).unwrap();
        for entry in std::fs::read_dir(source_path).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            std::fs::copy(entry.path(), dest_path.join(file_name)).unwrap();
        }
    }
}
