use std::collections::BTreeSet;
use std::ffi::OsStr;

use stem_core::LaunchTarget;

use super::open::Opener;
use super::read::{ReadOutcome, Refusal, decode};
use super::template::TEMPLATE;
use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stem-user-css-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every `--stem-<name>` variable in `text`.
fn variables(text: &str) -> BTreeSet<String> {
    const PREFIX: &str = "--stem-";
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        let tail = &rest[at + PREFIX.len()..];
        let end = tail
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .unwrap_or(tail.len());
        let name = tail[..end].trim_end_matches('-');
        if !name.is_empty() {
            names.insert(format!("{PREFIX}{name}"));
        }
        rest = &tail[end..];
    }
    names
}

#[test]
fn a_missing_file_is_no_css_and_a_utf8_one_is_its_text_without_a_bom() {
    let dir = scratch_dir("read");
    let file = dir.join(FILE_NAME);
    assert_eq!(read_file(&file), ReadOutcome::Missing);

    std::fs::write(&file, ".dock-stage { --stem-dot: red; }").unwrap();
    assert_eq!(
        read_file(&file),
        ReadOutcome::Css(".dock-stage { --stem-dot: red; }".to_owned())
    );

    std::fs::write(&file, "\u{feff}.dock-stage { --stem-dot: red; }").unwrap();
    assert_eq!(
        read_file(&file),
        ReadOutcome::Css(".dock-stage { --stem-dot: red; }".to_owned())
    );

    std::fs::write(&file, "").unwrap();
    assert_eq!(read_file(&file), ReadOutcome::Css(String::new()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_over_64_kib_or_not_utf8_or_not_a_file_is_refused() {
    let dir = scratch_dir("refuse");
    let file = dir.join(FILE_NAME);

    let limit = usize::try_from(MAX_BYTES).unwrap();
    std::fs::write(&file, "a".repeat(limit)).unwrap();
    assert!(matches!(read_file(&file), ReadOutcome::Css(css) if css.len() == limit));
    std::fs::write(&file, "a".repeat(limit + 1)).unwrap();
    assert_eq!(
        read_file(&file),
        ReadOutcome::Refused(Refusal::TooLarge(MAX_BYTES + 1))
    );

    // UTF-16 (Notepad's "Unicode"), with its byte order mark.
    std::fs::write(&file, [0xff, 0xfe, b'a', 0, b'{', 0, b'}', 0]).unwrap();
    assert_eq!(read_file(&file), ReadOutcome::Refused(Refusal::NotUtf8));

    let folder = dir.join("folder.css");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(matches!(
        read_file(&folder),
        ReadOutcome::Refused(Refusal::Unreadable(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn decode_counts_the_bom_against_the_limit_and_drops_only_a_leading_one() {
    let limit = usize::try_from(MAX_BYTES).unwrap();
    let mut bytes = "\u{feff}".as_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(b'a', limit - 3));
    assert_eq!(decode(bytes.clone()).map(|css| css.len()), Ok(limit - 3));
    bytes.push(b'a');
    assert_eq!(decode(bytes), Err(Refusal::TooLarge(MAX_BYTES + 1)));
    assert_eq!(
        decode("a\u{feff}".as_bytes().to_vec()),
        Ok("a\u{feff}".to_owned())
    );
    assert_eq!(decode(vec![0xc3, 0x28]), Err(Refusal::NotUtf8));
}

#[test]
fn the_state_reads_only_when_asked_and_keeps_the_last_read() {
    let dir = scratch_dir("state");
    let state = UserCssState::new(Some(&dir.join("settings.json")), false);
    let path = dir.join(FILE_NAME).display().to_string();
    let none = UserCss {
        path: path.clone(),
        css: None,
        problem: Some(UserCssProblem::NotFound),
    };
    assert_eq!(state.current(), none);

    std::fs::write(dir.join(FILE_NAME), "a {}").unwrap();
    assert_eq!(state.current(), none, "no read before reload");
    let read = state.reload();
    assert_eq!(read.css.as_deref(), Some("a {}"));
    assert_eq!(read.problem, None);
    assert_eq!(state.current(), read);

    // An edit shows only at the next reload; a refused file clears it
    // and says why.
    std::fs::write(dir.join(FILE_NAME), [0xff, 0xfe]).unwrap();
    assert_eq!(state.current(), read);
    let refused = UserCss {
        problem: Some(UserCssProblem::NotUtf8),
        ..none.clone()
    };
    assert_eq!(state.reload(), refused);
    assert_eq!(state.current(), refused);
    std::fs::remove_file(dir.join(FILE_NAME)).unwrap();
    assert_eq!(state.reload(), none);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn each_read_says_why_there_is_no_css() {
    let dir = scratch_dir("problem");
    let state = UserCssState::new(Some(&dir.join("settings.json")), false);
    let file = dir.join(FILE_NAME);
    let problem = |state: &UserCssState| {
        let read = state.reload();
        assert_eq!(read.css.is_none(), read.problem.is_some(), "{read:?}");
        read.problem
    };
    assert_eq!(problem(&state), Some(UserCssProblem::NotFound));
    std::fs::write(&file, "a {}").unwrap();
    assert_eq!(problem(&state), None);
    let limit = usize::try_from(MAX_BYTES).unwrap();
    std::fs::write(&file, "a".repeat(limit + 1)).unwrap();
    assert_eq!(problem(&state), Some(UserCssProblem::TooLarge));
    std::fs::write(&file, [0xff, 0xfe, b'a', 0]).unwrap();
    assert_eq!(problem(&state), Some(UserCssProblem::NotUtf8));
    // A folder by the file's name exists but is no file to read.
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir_all(&file).unwrap();
    assert_eq!(problem(&state), Some(UserCssProblem::Unreadable));
    std::fs::remove_dir_all(&dir).unwrap();

    assert_eq!(
        Refusal::Unreadable("denied".to_owned()).problem(),
        UserCssProblem::Unreadable
    );
    assert_eq!(Refusal::NotUtf8.problem(), UserCssProblem::NotUtf8);
    assert_eq!(
        Refusal::TooLarge(MAX_BYTES + 1).problem(),
        UserCssProblem::TooLarge
    );
}

#[test]
fn without_a_settings_file_there_is_no_dock_css() {
    for (demo, note) in [(true, NO_FILE_DEMO), (false, NO_FILE)] {
        let state = UserCssState::new(None, demo);
        let expected = UserCss {
            path: note.to_owned(),
            css: None,
            problem: Some(UserCssProblem::NotFound),
        };
        assert_eq!(state.current(), expected);
        assert_eq!(state.reload(), expected);
        assert_eq!(
            state.file_to_open(),
            Err("settings are not saved to a file in this run".to_owned())
        );
    }
    assert_eq!(path_for(Some(Path::new("settings.json"))), None);
}

#[test]
fn open_writes_the_template_once_and_never_over_a_file() {
    let dir = scratch_dir("open");
    let state = UserCssState::new(Some(&dir.join("settings.json")), false);
    let file = dir.join(FILE_NAME);
    assert_eq!(state.file_to_open(), Ok(file.clone()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), TEMPLATE);

    std::fs::write(&file, "mine").unwrap();
    assert_eq!(state.file_to_open(), Ok(file.clone()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");

    let gone = UserCssState::new(Some(&dir.join("missing").join("settings.json")), false);
    assert!(
        gone.file_to_open()
            .is_err_and(|e| e.starts_with("could not create ")),
        "a folder that does not exist"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_template_changes_nothing_and_lists_the_stable_variables_of_theming_md() {
    // Every declaration is commented out: the template's CSS, comments
    // removed, is one empty rule.
    let mut css = String::new();
    let mut rest = TEMPLATE;
    while let Some(start) = rest.find("/*") {
        css.push_str(&rest[..start]);
        let end = rest[start + 2..]
            .find("*/")
            .expect("every comment in the template is closed");
        rest = &rest[start + 2 + end + 2..];
    }
    css.push_str(rest);
    let css: String = css.split_whitespace().collect();
    assert_eq!(css, ".dock-stage{}");

    let theming = include_str!("../../../docs/THEMING.md");
    assert_eq!(variables(TEMPLATE), variables(theming));
    assert_eq!(variables(TEMPLATE).len(), 9);
    assert!(decode(TEMPLATE.as_bytes().to_vec()).is_ok());
}

#[test]
fn open_tries_the_default_app_then_notepad_and_names_both_failures() {
    let file = Path::new(r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css");
    let notepad = notepad_for(file, Some(OsStr::new(r"C:\WINDOWS")));
    assert_eq!(
        notepad,
        LaunchTarget::Exe {
            path: PathBuf::from(r"C:\WINDOWS\System32\notepad.exe"),
            args: vec![file.to_string_lossy().into_owned()],
            working_dir: None,
        }
    );
    let uri = LaunchTarget::Uri {
        uri: file.to_string_lossy().into_owned(),
    };

    // The default app opens it: Notepad is never tried.
    let mut tried = Vec::new();
    let opened = open_file(file, &notepad, |target| {
        tried.push(target.clone());
        Ok(())
    });
    assert_eq!(opened, Ok(Opener::DefaultApp));
    assert_eq!(tried, std::slice::from_ref(&uri));

    // No app for `.css` (the shell refuses without UI): Notepad.
    let mut tried = Vec::new();
    let opened = open_file(file, &notepad, |target| {
        tried.push(target.clone());
        match target {
            LaunchTarget::Uri { .. } => Err("no association".to_owned()),
            _ => Ok(()),
        }
    });
    assert_eq!(opened, Ok(Opener::Notepad));
    assert_eq!(tried, [uri.clone(), notepad.clone()]);

    // Both fail: one error naming the file and both reasons.
    let opened = open_file(file, &notepad, |target| {
        Err(match target {
            LaunchTarget::Uri { .. } => "no association".to_owned(),
            _ => "not found".to_owned(),
        })
    });
    assert_eq!(
        opened,
        Err(format!(
            "could not open {} with its default app (no association) or with Notepad (not found)",
            file.display()
        ))
    );
    assert_eq!(Opener::DefaultApp.to_string(), "with its default app");
    assert_eq!(Opener::Notepad.to_string(), "in Notepad");
}

#[test]
fn notepad_is_under_system_root_when_it_is_known() {
    let file = Path::new(r"C:\Users\me\dock.css");
    let path_of = |root: Option<&str>| match notepad_for(file, root.map(OsStr::new)) {
        LaunchTarget::Exe { path, .. } => path.to_string_lossy().into_owned(),
        other => panic!("not an exe: {other:?}"),
    };
    assert_eq!(
        path_of(Some(r"C:\Windows\")),
        r"C:\Windows\System32\notepad.exe"
    );
    assert_eq!(path_of(Some(r" D:\Win ")), r"D:\Win\System32\notepad.exe");
    assert_eq!(path_of(Some("")), "notepad.exe");
    assert_eq!(path_of(None), "notepad.exe");
}

#[test]
fn a_reload_waits_for_the_one_before_it_to_publish() {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;

    let dir = scratch_dir("serial");
    let file = dir.join(FILE_NAME);
    let state = Arc::new(UserCssState::new(Some(&dir.join("settings.json")), false));
    let published = Arc::new(StdMutex::new(Vec::<Option<String>>::new()));
    std::fs::write(&file, "old {}").unwrap();

    // The first reload has read "old {}" and is publishing; it waits
    // there until released.
    let (in_publish_tx, in_publish) = mpsc::channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let first = {
        let (state, published) = (Arc::clone(&state), Arc::clone(&published));
        std::thread::spawn(move || {
            state.reload_and(|css| {
                in_publish_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                published.lock().unwrap().push(css.css.clone());
            })
        })
    };
    in_publish.recv().unwrap();

    // A save and a second Reload meanwhile: it must not read, store or
    // publish before the first has published.
    std::fs::write(&file, "new {}").unwrap();
    let second = {
        let (state, published) = (Arc::clone(&state), Arc::clone(&published));
        std::thread::spawn(move || {
            state.reload_and(|css| published.lock().unwrap().push(css.css.clone()))
        })
    };
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !second.is_finished(),
        "the second reload ran past the first"
    );
    assert!(published.lock().unwrap().is_empty());

    release.send(()).unwrap();
    assert_eq!(first.join().unwrap().css.as_deref(), Some("old {}"));
    assert_eq!(second.join().unwrap().css.as_deref(), Some("new {}"));
    assert_eq!(
        *published.lock().unwrap(),
        [Some("old {}".to_owned()), Some("new {}".to_owned())]
    );
    assert_eq!(state.current().css.as_deref(), Some("new {}"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_event_goes_to_dock_windows_by_label() {
    for label in ["dock", "dock-DISPLAY3"] {
        assert!(is_dock_target(&EventTarget::WebviewWindow {
            label: label.to_owned()
        }));
        assert!(is_dock_target(&EventTarget::AnyLabel {
            label: label.to_owned()
        }));
    }
    assert!(!is_dock_target(&EventTarget::WebviewWindow {
        label: "settings".to_owned()
    }));
    assert!(!is_dock_target(&EventTarget::App));
    assert!(!is_dock_target(&EventTarget::Any));
}
