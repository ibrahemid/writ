use std::process::Command;

#[test]
fn help_describes_the_title_flag_as_the_piped_file_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_writ"))
        .arg("--help")
        .output()
        .expect("failed to run the writ binary");
    assert!(
        output.status.success(),
        "writ --help exited with {:?}",
        output.status.code()
    );

    let help = String::from_utf8(output.stdout).expect("writ --help wrote non-UTF-8 output");
    let title_line = help
        .lines()
        .find(|line| line.trim_start().starts_with("--title"))
        .unwrap_or_else(|| panic!("no --title line in:\n{help}"));
    assert!(
        title_line.contains("Name of the file piped stdin is saved to"),
        "unexpected --title help: {title_line}"
    );
    assert!(!help.contains("buffer"), "help still says buffer:\n{help}");
}
