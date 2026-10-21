//! Downloads EmbeddingGemma 2 (SHA-256 pinned, resumable) into a models directory.
//!   cargo run --release -p lowk-index --example install_model -- <models dir>
use lowk_index::embedder::models::ModelStore;
use lowk_index::embedder::{gemma_assets, GEMMA_ENGINE_ID};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("models dir"));
    let st = ModelStore::new(root);
    let specs = gemma_assets();
    let mut last = 0u64;
    st.install(
        GEMMA_ENGINE_ID,
        &specs,
        &AtomicBool::new(false),
        &mut |f, phase, done, total| {
            if done / (20 << 20) != last / (20 << 20) || done == total {
                println!("{f}: {phase:?} {} / {} MB", done >> 20, total >> 20);
            }
            last = done;
        },
    )
    .expect("install");
    println!("installed and verified: {}", st.verify(GEMMA_ENGINE_ID, &specs));
    println!("{}", st.path(GEMMA_ENGINE_ID, &specs[0].file).display());
}
