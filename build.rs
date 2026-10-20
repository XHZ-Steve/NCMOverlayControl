use std::{collections::BTreeMap, env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=licenses");
    println!("cargo:rerun-if-changed=THIRD-PARTY-NOTICES.txt");
    println!("cargo:rerun-if-changed=LICENSE");
    let mut unique = BTreeMap::<String, Vec<String>>::new();
    for directory in fs::read_dir("licenses").unwrap() {
        let directory = directory.unwrap();
        if !directory.path().is_dir() {
            continue;
        }
        let name = directory.file_name().to_string_lossy().to_string();
        for file in fs::read_dir(directory.path()).unwrap() {
            let file = file.unwrap();
            if file.path().is_file() {
                let content = fs::read_to_string(file.path()).unwrap();
                unique.entry(content).or_default().push(name.clone());
            }
        }
    }
    let mut result = fs::read_to_string("THIRD-PARTY-NOTICES.txt").unwrap();
    result.push_str("\n\n===== NCM Overlay Control =====\n");
    result.push_str(&fs::read_to_string("LICENSE").unwrap());
    for (content, names) in unique {
        result.push_str(&format!(
            "\n\n===== {} =====\n{}",
            names.join(", "),
            content
        ));
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("licenses.txt"),
        result,
    )
    .unwrap();
}
