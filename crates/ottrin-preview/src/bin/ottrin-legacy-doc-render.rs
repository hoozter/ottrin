use std::path::PathBuf;

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let [input_flag, input, output_flag, output] = arguments.as_slice() else {
        fail("usage: ottrin-legacy-doc-render --input <path> --output <path>");
    };
    if input_flag != "--input" || output_flag != "--output" {
        fail("usage: ottrin-legacy-doc-render --input <path> --output <path>");
    }

    match ottrin_preview::run_legacy_doc_render_helper(
        &PathBuf::from(input),
        &PathBuf::from(output),
    ) {
        Ok(page_count) => println!("{page_count}"),
        Err(error) => fail(&error.to_string()),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("ottrin-legacy-doc-render: {message}");
    std::process::exit(1)
}
