use pretty_assertions::assert_eq;

use super::*;

fn strings(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|&p| p.to_owned()).collect()
}

fn everything_exists(paths: &[&str]) -> RecyclePlan {
    plan_recycle(&strings(paths), |_| true)
}

fn skipped(path: &str, reason: SkipReason) -> Skipped {
    Skipped {
        path: path.to_owned(),
        reason,
    }
}

#[test]
fn keeps_drive_and_unc_paths_in_the_shells_form() {
    let plan = everything_exists(&[
        r"C:\Users\me\Desktop\notes.txt",
        "C:/Users/me/Desktop/Folder/",
        r"d:\Data\\archive.zip",
        r"\\nas\share\photos\2024",
        "//nas/share/report.pdf",
    ]);
    assert_eq!(
        plan.recycle,
        strings(&[
            r"C:\Users\me\Desktop\notes.txt",
            r"C:\Users\me\Desktop\Folder",
            r"d:\Data\archive.zip",
            r"\\nas\share\photos\2024",
            r"\\nas\share\report.pdf",
        ])
    );
    assert_eq!(plan.skipped, vec![]);
}

#[test]
fn leaves_out_what_is_not_a_plain_absolute_file_path() {
    let odd = [
        "",
        "notes.txt",
        r"Documents\notes.txt",
        r"\Users\me\notes.txt",
        "C:notes.txt",
        "C:",
        r"\\?\C:\Users\me\notes.txt",
        r"\\.\pipe\stem",
        r"\\?\UNC\nas\share\x",
        r"C:\Users\me\..\..\Windows",
        r"C:\Users\.\me",
        r"C:\Users\me\*.txt",
        r"C:\Users\me\notes?.txt",
        r"C:\Users\me\notes.txt:secret",
        "C:\\Users\\me\0\\notes.txt",
        r"\\\nas\share\x",
        "::{645FF040-5081-101B-9F08-00AA002F954E}",
        "shell:RecycleBinFolder",
    ];
    let plan = everything_exists(&odd);
    assert_eq!(plan.recycle, Vec::<String>::new());
    assert_eq!(
        plan.skipped,
        odd.iter()
            .map(|p| skipped(p, SkipReason::NotAFilePath))
            .collect::<Vec<_>>()
    );
}

#[test]
fn never_asks_for_a_drive_or_share_root() {
    let roots = [
        r"C:\",
        "C:/",
        r"C:\\",
        r"\\nas",
        r"\\nas\",
        r"\\nas\share",
        r"\\nas\share\",
    ];
    let plan = everything_exists(&roots);
    assert_eq!(plan.recycle, Vec::<String>::new());
    let reasons: Vec<SkipReason> = plan.skipped.iter().map(|s| s.reason).collect();
    assert_eq!(
        reasons,
        vec![
            SkipReason::Root,
            SkipReason::Root,
            SkipReason::Root,
            SkipReason::Root,
            SkipReason::Root,
            SkipReason::Root,
            SkipReason::Root,
        ]
    );
}

#[test]
fn asks_whether_each_plain_path_exists_in_the_shells_form() {
    let asked = std::cell::RefCell::new(Vec::new());
    let plan = plan_recycle(
        &strings(&["C:/a/here.txt", "C:/a/gone.txt", "relative.txt"]),
        |p| {
            asked.borrow_mut().push(p.to_owned());
            !p.ends_with("gone.txt")
        },
    );
    assert_eq!(
        *asked.borrow(),
        strings(&[r"C:\a\here.txt", r"C:\a\gone.txt"])
    );
    assert_eq!(plan.recycle, strings(&[r"C:\a\here.txt"]));
    assert_eq!(
        plan.skipped,
        vec![
            skipped("C:/a/gone.txt", SkipReason::Missing),
            skipped("relative.txt", SkipReason::NotAFilePath),
        ]
    );
}

#[test]
fn lists_each_path_once_and_lets_a_folder_take_its_contents_along() {
    let plan = everything_exists(&[
        r"C:\Work\report.docx",
        r"C:\Work\Drafts\one.txt",
        "c:/work/REPORT.DOCX",
        r"C:\Work\Drafts",
        r"C:\Work\Drafts2\two.txt",
        r"C:\Work\Drafts\",
    ]);
    assert_eq!(
        plan.recycle,
        strings(&[
            r"C:\Work\report.docx",
            r"C:\Work\Drafts",
            r"C:\Work\Drafts2\two.txt"
        ])
    );
    assert_eq!(
        plan.skipped,
        vec![
            skipped(r"C:\Work\Drafts\one.txt", SkipReason::InsideDropped),
            skipped("c:/work/REPORT.DOCX", SkipReason::Duplicate),
            skipped(r"C:\Work\Drafts\", SkipReason::Duplicate),
        ]
    );
}

#[test]
fn only_refusals_and_missing_paths_are_warnings() {
    assert!(SkipReason::NotAFilePath.is_warning());
    assert!(SkipReason::Root.is_warning());
    assert!(SkipReason::Missing.is_warning());
    assert!(!SkipReason::Duplicate.is_warning());
    assert!(!SkipReason::InsideDropped.is_warning());
}

#[test]
fn says_why_nothing_goes_to_the_bin() {
    let none = |paths: &[&str], exists: bool| {
        nothing_to_recycle(&plan_recycle(&strings(paths), |_| exists))
    };
    assert_eq!(
        nothing_to_recycle(&RecyclePlan::default()),
        "no paths to move to the Recycle Bin"
    );
    assert_eq!(
        none(&[r"C:\Users\me\notes.txt"], false),
        r"C:\Users\me\notes.txt does not exist"
    );
    assert_eq!(none(&[r"C:\"], true), r"C:\ is a drive or share root");
    assert_eq!(
        none(&["notes.txt"], true),
        "notes.txt is not an absolute file path"
    );
    assert_eq!(
        none(&[r"C:\a.txt", r"C:\b.txt", r"c:\A.TXT"], false),
        "none of the 3 paths exists"
    );
    assert_eq!(
        none(&[r"C:\a.txt", "b.txt"], false),
        "none of the 2 paths can be moved to the Recycle Bin"
    );
}

#[test]
fn the_bin_state_is_the_shells_count_and_size_and_never_negative() {
    let state = |item_count, size_bytes| RecycleBinState {
        item_count,
        size_bytes,
    };
    assert_eq!(bin_state(0, 0), state(0, 0));
    assert_eq!(bin_state(3, 4096), state(3, 4096));
    assert_eq!(
        bin_state(i64::MAX, i64::MAX),
        state(i64::MAX as u64, i64::MAX as u64)
    );
    assert_eq!(bin_state(-1, 4096), state(0, 4096));
    assert_eq!(bin_state(3, i64::MIN), state(3, 0));
}
