fn main() {
    embed_manifest::embed_manifest_file("app.manifest")
        .expect("embed Windows application manifest");
    println!("cargo:rerun-if-changed=app.manifest");
}
