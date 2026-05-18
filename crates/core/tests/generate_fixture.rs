use pdf_oxide::writer::DocumentBuilder;
use std::path::PathBuf;

fn main() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixture_dir).unwrap();

    let mut builder = DocumentBuilder::new();

    builder
        .letter_page()
        .font("Helvetica", 24.0)
        .at(72.0, 72.0)
        .text("The Founders Playbook")
        .font("Helvetica", 18.0)
        .space(20.0)
        .text("Chapter 1: Getting Started")
        .font("Helvetica", 12.0)
        .space(16.0)
        .paragraph("Building a successful startup requires a clear vision, relentless execution, and the ability to adapt quickly to changing market conditions. This guide will walk you through the essential steps of launching and growing your company.")
        .space(16.0)
        .font("Helvetica", 16.0)
        .text("Key Principles")
        .font("Helvetica", 12.0)
        .space(12.0)
        .paragraph("1. Start with a problem worth solving. The best companies are built around genuine pain points that affect a large number of people.")
        .space(8.0)
        .paragraph("2. Build a minimum viable product (MVP) and get it into the hands of real users as quickly as possible. Feedback is your most valuable resource.")
        .space(8.0)
        .paragraph("3. Measure everything. Data-driven decisions separate successful founders from those who rely on gut feeling alone.");

    let bytes = builder.build().expect("Failed to build PDF");
    let output_path = fixture_dir.join("sample.pdf");
    std::fs::write(&output_path, &bytes).unwrap();
    println!("Generated: {} ({} bytes)", output_path.display(), bytes.len());
}
