use std::path::Path;

use hallucinator_parsing::ReferenceExtractor;
use hallucinator_pdf_mupdf::MupdfBackend;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: cargo run --example locate -- <pdf>");
    let extractor = ReferenceExtractor::new();
    let backend = MupdfBackend;
    let result = extractor
        .extract_references_with_locations(Path::new(&path), &backend)
        .expect("extract failed");
    let mut located = 0;
    for r in &result.references {
        let title = r.title.as_deref().unwrap_or("(no title)");
        let title_short = &title[..title.len().min(70)];
        match (r.page_number, r.bboxes.first()) {
            (Some(p), Some(b)) => {
                located += 1;
                println!(
                    "  located ref{:>3}  page {}  ({:.0},{:.0})-({:.0},{:.0})  {} bboxes  {}",
                    r.original_number,
                    p,
                    b.x0,
                    b.y0,
                    b.x1,
                    b.y1,
                    r.bboxes.len(),
                    title_short
                );
            }
            _ => {
                println!(
                    "  MISSED  ref{:>3}                                                  {}",
                    r.original_number, title_short
                );
            }
        }
    }
    println!(
        "\n  total: {} located / {} references",
        located,
        result.references.len()
    );
}
