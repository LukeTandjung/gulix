//! Tern Surface Protocol frontend spike.

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _runtime_context = runtime.enter();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    helix_term::tern_frontend::run(path.as_deref())
}
