mod common;

use common::TempDir;
use sc_core::Error;
use sc_core::folder::{
    LocalDateTime, MAX_TITLE_BYTES, PROJECT_SUFFIX, ProjectLock, create_project_dir,
    is_project_dir, project_folder_name, rename_project_dir, rfc3339,
};

const T: LocalDateTime = LocalDateTime {
    y: 2026,
    mo: 11,
    d: 9,
    h: 7,
    mi: 5,
    s: 3,
    offset_min: 60,
};

#[test]
fn folder_names() {
    assert_eq!(
        project_folder_name(&T, "Demo"),
        "2026-11-09 07.05.03 Demo.sc"
    );
    assert_eq!(
        project_folder_name(&T, "a/b\\c:d*e?f\"g<h>i|j"),
        "2026-11-09 07.05.03 a-b-c-d-e-f-g-h-i-j.sc"
    );
    assert_eq!(
        project_folder_name(&T, "Meeting 10:30 / notes"),
        "2026-11-09 07.05.03 Meeting 10-30 - notes.sc"
    );
    assert_eq!(
        project_folder_name(&T, "  tabs\tand\nnewlines  "),
        "2026-11-09 07.05.03 tabs and newlines.sc"
    );
    assert_eq!(
        project_folder_name(&T, "Emoji 🎬👩‍💻 ok"),
        "2026-11-09 07.05.03 Emoji 🎬👩‍💻 ok.sc"
    );
    assert_eq!(
        project_folder_name(&T, ""),
        "2026-11-09 07.05.03 Recording.sc"
    );
    assert_eq!(
        project_folder_name(&T, " ..."),
        "2026-11-09 07.05.03 Recording.sc"
    );
    assert_eq!(
        project_folder_name(&T, "end."),
        "2026-11-09 07.05.03 end.sc"
    );
}

#[test]
fn long_titles_are_capped_at_a_char_boundary() {
    for title in ["x".repeat(300), "é".repeat(300), "👩‍💻".repeat(300)] {
        let name = project_folder_name(&T, &title);
        let t = name
            .strip_prefix("2026-11-09 07.05.03 ")
            .and_then(|n| n.strip_suffix(PROJECT_SUFFIX))
            .expect("shape");
        assert!(!t.is_empty() && t.len() <= MAX_TITLE_BYTES, "{}", t.len());
        assert!(name.len() < 255);
        assert!(!t.ends_with('\u{200D}'));
    }
}

#[test]
fn create_dir_avoids_collisions_and_is_recognised() {
    let root = TempDir::new("folders");
    let movies = root.path().join("Movies/Simple Capture");
    let a = create_project_dir(&movies, &T, "Demo").expect("first");
    let b = create_project_dir(&movies, &T, "Demo").expect("second");
    let c = create_project_dir(&movies, &T, "Demo").expect("third");
    assert_eq!(
        a.file_name().and_then(|n| n.to_str()),
        Some("2026-11-09 07.05.03 Demo.sc")
    );
    assert_eq!(
        b.file_name().and_then(|n| n.to_str()),
        Some("2026-11-09 07.05.03 Demo 2.sc")
    );
    assert_eq!(
        c.file_name().and_then(|n| n.to_str()),
        Some("2026-11-09 07.05.03 Demo 3.sc")
    );
    assert!(!is_project_dir(&a), "empty folder is not a project yet");
    std::fs::write(a.join("journal.jsonl"), "").expect("journal");
    assert!(is_project_dir(&a));
    std::fs::write(b.join("project.json"), "{}").expect("project");
    assert!(is_project_dir(&b));
    let other = movies.join("notes");
    std::fs::create_dir(&other).expect("mkdir");
    std::fs::write(other.join("project.json"), "{}").expect("project");
    assert!(!is_project_dir(&other));
    assert!(!is_project_dir(&b.join("project.json")));
}

#[test]
fn rfc3339_formats_offsets() {
    assert_eq!(rfc3339(&T), "2026-11-09T07:05:03+01:00");
    let west = LocalDateTime {
        offset_min: -(5 * 60 + 30),
        ..T
    };
    assert_eq!(rfc3339(&west), "2026-11-09T07:05:03-05:30");
    let utc = LocalDateTime { offset_min: 0, ..T };
    assert_eq!(rfc3339(&utc), "2026-11-09T07:05:03+00:00");
}

#[test]
fn from_unix_matches_known_dates() {
    let t = LocalDateTime::from_unix(0, 0);
    assert_eq!(rfc3339(&t), "1970-01-01T00:00:00+00:00");
    // 2026-11-19T10:00:00Z
    let t = LocalDateTime::from_unix(1_795_082_400, 0);
    assert_eq!(rfc3339(&t), "2026-11-19T10:00:00+00:00");
    let t = LocalDateTime::from_unix(1_795_082_400, -8 * 60);
    assert_eq!(rfc3339(&t), "2026-11-19T02:00:00-08:00");
    // Leap day.
    let t = LocalDateTime::from_unix(951_782_400, 0);
    assert_eq!(rfc3339(&t), "2000-02-29T00:00:00+00:00");
    let now = LocalDateTime::now();
    assert!(now.y >= 2025 && (1..=12).contains(&now.mo) && (1..=31).contains(&now.d));
    assert!(now.offset_min.abs() <= 14 * 60);
}

#[test]
fn lock_is_exclusive_in_process_and_released_on_drop() {
    let dir = TempDir::new("lock");
    let l = ProjectLock::acquire(dir.path()).expect("acquire");
    assert!(l.path().exists());
    assert!(matches!(
        ProjectLock::acquire(dir.path()),
        Err(Error::Locked(_))
    ));
    drop(l);
    assert!(!dir.path().join(".lock").exists());
    let _again = ProjectLock::acquire(dir.path()).expect("reacquire");
}

/// Helper body: takes the lock, says so, holds it until stdin closes.
#[test]
fn helper_lock_holder() {
    let Ok(dir) = std::env::var("SC_CORE_LOCK_HELPER") else {
        return;
    };
    let lock = ProjectLock::acquire(std::path::Path::new(&dir)).expect("helper acquire");
    println!("\nLOCKED");
    let mut sink = String::new();
    let _ = std::io::stdin().read_line(&mut sink);
    drop(lock);
}

#[test]
fn lock_contention_between_two_processes() {
    let dir = TempDir::new("lock2");
    let d = dir.path().to_str().expect("utf8 path");
    let mut child = common::spawn_helper("helper_lock_holder", &[("SC_CORE_LOCK_HELPER", d)]);
    common::wait_for_line(&mut child, "LOCKED");
    match ProjectLock::acquire(dir.path()) {
        Err(Error::Locked(p)) => assert_eq!(p, dir.path().join(".lock")),
        other => panic!("expected Locked while the helper holds it, got {other:?}"),
    }
    drop(child.stdin.take());
    assert!(child.wait().expect("helper exit").success());
    let _l = ProjectLock::acquire(dir.path()).expect("free after helper released it");
}

#[cfg(unix)]
#[test]
fn lock_of_a_killed_process_is_stale() {
    let dir = TempDir::new("lock3");
    let d = dir.path().to_str().expect("utf8 path");
    let mut child = common::spawn_helper("helper_lock_holder", &[("SC_CORE_LOCK_HELPER", d)]);
    common::wait_for_line(&mut child, "LOCKED");
    child.kill().expect("SIGKILL helper");
    child.wait().expect("reap");
    assert!(
        dir.path().join(".lock").exists(),
        "killed holder leaves its lock"
    );
    let _l = ProjectLock::acquire(dir.path()).expect("stale lock taken over");
}

#[test]
fn fresh_unreadable_lock_counts_as_held() {
    let dir = TempDir::new("lock4");
    std::fs::write(dir.path().join(".lock"), "").expect("write");
    assert!(matches!(
        ProjectLock::acquire(dir.path()),
        Err(Error::Locked(_))
    ));
}

#[test]
fn renaming_keeps_the_date_and_steps_around_other_projects() {
    let root = TempDir::new("rename");
    let movies = root.path().join("Simple Capture");
    let a = create_project_dir(&movies, &T, "Recording").expect("a");
    let b = create_project_dir(&movies, &T, "Demo").expect("b");
    std::fs::write(a.join("project.json"), "{}").expect("project");
    let name = |p: &std::path::Path| p.file_name().and_then(|n| n.to_str()).map(str::to_owned);

    let a2 = rename_project_dir(&a, "Q4: plan / review").expect("rename");
    assert_eq!(
        name(&a2).as_deref(),
        Some("2026-11-09 07.05.03 Q4- plan - review.sc")
    );
    assert!(a2.join("project.json").exists() && !a.exists());

    assert_eq!(
        rename_project_dir(&a2, "Q4: plan / review").expect("same"),
        a2
    );

    let a3 = rename_project_dir(&a2, "Demo").expect("taken");
    assert_eq!(name(&a3).as_deref(), Some("2026-11-09 07.05.03 Demo 2.sc"));
    assert!(b.exists());
    assert_eq!(rename_project_dir(&a3, "Demo").expect("own suffix"), a3);

    let mine = movies.join("My take.sc");
    std::fs::create_dir(&mine).expect("mkdir");
    assert_eq!(rename_project_dir(&mine, "Other").expect("undated"), mine);
}
