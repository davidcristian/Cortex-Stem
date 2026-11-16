use pretty_assertions::assert_eq;

use super::*;

#[test]
fn command_line_splitting_follows_msvcrt_rules() {
    assert_eq!(split_command_line(""), Vec::<String>::new());
    assert_eq!(split_command_line("   "), Vec::<String>::new());
    assert_eq!(split_command_line("a b  c"), vec!["a", "b", "c"]);
    assert_eq!(
        split_command_line(r#""C:\Program Files\x.exe" --flag "two words""#),
        vec![r"C:\Program Files\x.exe", "--flag", "two words"]
    );
    assert_eq!(split_command_line(r#"a\\b"#), vec![r"a\\b"]);
    assert_eq!(split_command_line(r#"a\\"b c"#), vec![r"a\b c"]);
    assert_eq!(split_command_line(r#"a\"b c"#), vec![r#"a"b"#, "c"]);
    assert_eq!(split_command_line(r#""a""b" c"#), vec![r#"a"b"#, "c"]);
    assert_eq!(split_command_line(r#""" x"#), vec!["", "x"]);
    assert_eq!(
        split_command_line(r#"--profile-directory="Profile 1""#),
        vec!["--profile-directory=Profile 1"]
    );
}

#[test]
fn command_line_join_round_trips() {
    let cases: Vec<Vec<String>> = vec![
        vec![],
        vec!["plain".into()],
        vec!["".into(), "x".into()],
        vec!["two words".into(), r"C:\path\".into()],
        vec![r#"say "hi""#.into(), r#"back\slash\"quote"#.into()],
        vec![r"trailing\\".into(), "tab\there".into()],
    ];
    for args in cases {
        let line = join_command_line(&args);
        assert_eq!(split_command_line(&line), args, "line: {line}");
    }
    assert_eq!(quote_argument("plain"), "plain");
    assert_eq!(quote_argument(""), r#""""#);
    assert_eq!(quote_argument("two words"), r#""two words""#);
    // A trailing backslash is literal unless a quote follows it.
    assert_eq!(quote_argument(r"C:\dir\"), r"C:\dir\");
    assert_eq!(quote_argument(r"C:\my dir\"), r#""C:\my dir\\""#);
    assert_eq!(join_command_line(&["a".into(), "b c".into()]), r#"a "b c""#);
}
