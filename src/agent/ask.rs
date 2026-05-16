use anyhow::Result;
use std::path::Path;

use crate::config::Config;
use crate::llm::{openai, ChatMessage, GenOpts};
use crate::rag::{bm25::Bm25Index, scan::scan_repo, Retriever};

use super::system_preamble;

pub async fn run(cfg: &Config, repo: &Path, question: &str, k: usize) -> Result<()> {
    eprintln!("indexing {} ...", repo.display());
    let chunks = scan_repo(repo)?;
    let n = chunks.len();
    let index = Bm25Index::build(chunks);
    eprintln!("indexed {} chunks; retrieving top {}", n, k);

    let hits = index.search(question, k);
    if hits.is_empty() {
        println!("(no relevant context found — answering from model knowledge only)");
    } else {
        eprintln!("context:");
        for (chunk, score) in &hits {
            eprintln!(
                "  [{:.2}] {}:{}-{}",
                score,
                chunk.path.display(),
                chunk.start_line,
                chunk.end_line
            );
        }
    }

    let context = render_context(&hits);
    let backend = openai::build(cfg);
    let msgs = vec![
        ChatMessage::system(system_preamble(cfg)),
        ChatMessage::user(format!(
            "Codebase context:\n\n{context}\n\nQuestion: {question}\n\nAnswer concisely. \
             Cite file paths and line ranges where relevant."
        )),
    ];
    let answer = backend.chat(&msgs, &GenOpts::default()).await?;
    println!("{answer}");
    Ok(())
}

fn render_context(hits: &[(crate::rag::Chunk, f32)]) -> String {
    let mut s = String::new();
    for (chunk, _) in hits {
        s.push_str(&format!(
            "--- {}:{}-{} ---\n{}\n\n",
            chunk.path.display(),
            chunk.start_line,
            chunk.end_line,
            chunk.text
        ));
    }
    s
}
