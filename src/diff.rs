use console::{style, Style};
use similar::{ChangeTag, TextDiff};

/// Print a colored unified diff to stdout.
pub fn print_diff(label: &str, old: &str, new: &str) {
    let diff = TextDiff::from_lines(old, new);
    println!("{}", style(format!("--- {} (old)", label)).red().bold());
    println!("{}", style(format!("+++ {} (new)", label)).green().bold());
    for group in diff.grouped_ops(3) {
        for op in group {
            for change in diff.iter_inline_changes(&op) {
                let (sign, style_) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::new().red()),
                    ChangeTag::Insert => ("+", Style::new().green()),
                    ChangeTag::Equal => (" ", Style::new().dim()),
                };
                print!("{}", style_.apply_to(sign));
                for (_, value) in change.iter_strings_lossy() {
                    print!("{}", style_.apply_to(value));
                }
                if change.missing_newline() { println!(); }
            }
        }
    }
}

/// Yes/no prompt on stderr; reads a line from stdin.
pub fn confirm(prompt: &str) -> std::io::Result<bool> {
    use std::io::{stderr, stdin, Write};
    eprint!("{} [y/N] ", prompt);
    stderr().flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}
