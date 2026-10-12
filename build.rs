fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        embed_resource::compile("unrar.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embed resources");
    }
}
