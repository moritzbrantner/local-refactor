pub fn unified(file_path: &str, original: &str, updated: &str) -> String {
    let mut output = String::new();
    output.push_str(&format!("--- {file_path}\n+++ {file_path}\n"));

    let original_lines: Vec<_> = original.lines().collect();
    let updated_lines: Vec<_> = updated.lines().collect();
    let max = original_lines.len().max(updated_lines.len());

    for index in 0..max {
        match (original_lines.get(index), updated_lines.get(index)) {
            (Some(left), Some(right)) if left == right => {
                output.push_str(&format!(" {left}\n"));
            }
            (Some(left), Some(right)) => {
                output.push_str(&format!("-{left}\n+{right}\n"));
            }
            (Some(left), None) => output.push_str(&format!("-{left}\n")),
            (None, Some(right)) => output.push_str(&format!("+{right}\n")),
            (None, None) => {}
        }
    }

    output
}
