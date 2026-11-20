//! Recovery scan (ARCHITECTURE §12 step 3): turns a project left in state `recording` by a crash
//! or kill into a `recovered` (or `damaged`) project, from `project.json`, `journal.jsonl` and a
//! probe of every track file. Track files and the journal are never modified or deleted; only
//! `project.json` is rewritten (atomically) and `session.log` is appended to.

mod rebuild;

use std::fs;
use std::path::{Path, PathBuf};

use sc_core::folder::{self, ProjectLock};
use sc_core::journal;
use sc_core::project::{ProjectState, ProjectV1};
use sc_core::sessionlog::{Level, SessionLog};
use sc_platform::Decoder;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Recovered,
    Damaged,
    AlreadyFinal,
    Locked,
    NotAProject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub dir: PathBuf,
    pub outcome: Outcome,
    pub segments: usize,
    /// Segment ids removed from the segment list; their files stay on disk.
    pub dropped_segments: Vec<String>,
    pub notes: Vec<String>,
}

impl RecoveryReport {
    fn new(dir: &Path, outcome: Outcome) -> RecoveryReport {
        RecoveryReport {
            dir: dir.to_path_buf(),
            outcome,
            segments: 0,
            dropped_segments: Vec::new(),
            notes: Vec::new(),
        }
    }
}

enum Loaded {
    Recording(Box<ProjectV1>),
    Missing,
    Final(usize),
    Unreadable(String),
}

fn load(dir: &Path) -> Loaded {
    if !dir.join(ProjectV1::FILE_NAME).exists() {
        return Loaded::Missing;
    }
    match ProjectV1::load(dir) {
        Ok(p) if p.state == ProjectState::Recording => Loaded::Recording(Box::new(p)),
        Ok(p) => Loaded::Final(p.recording.screen.segments.len()),
        Err(e) => Loaded::Unreadable(e.to_string()),
    }
}

fn left_alone(dir: &Path, loaded: &Loaded) -> Option<RecoveryReport> {
    match loaded {
        Loaded::Final(segments) => {
            let mut r = RecoveryReport::new(dir, Outcome::AlreadyFinal);
            r.segments = *segments;
            Some(r)
        }
        Loaded::Unreadable(e) => {
            let mut r = RecoveryReport::new(dir, Outcome::Damaged);
            r.notes
                .push(format!("project.json unreadable, nothing written: {e}"));
            Some(r)
        }
        Loaded::Recording(_) | Loaded::Missing => None,
    }
}

/// Recovers one project folder. Idempotent: a project that is no longer in state `recording`
/// reports `AlreadyFinal` and nothing is written.
pub fn recover_project(dir: &Path, decoder: &dyn Decoder) -> Result<RecoveryReport> {
    if !folder::is_project_dir(dir) {
        return Ok(RecoveryReport::new(dir, Outcome::NotAProject));
    }
    if let Some(r) = left_alone(dir, &load(dir)) {
        return Ok(r);
    }
    let _lock = match ProjectLock::acquire(dir) {
        Ok(lock) => lock,
        Err(sc_core::Error::Locked(_)) => {
            let mut r = RecoveryReport::new(dir, Outcome::Locked);
            r.notes
                .push("the project is locked by a live process".to_string());
            return Ok(r);
        }
        Err(e) => return Err(e.into()),
    };
    // The lock holder may have finalized the project between the first read and the lock.
    let loaded = load(dir);
    if let Some(r) = left_alone(dir, &loaded) {
        return Ok(r);
    }
    let project = match loaded {
        Loaded::Recording(p) => Some(*p),
        _ => None,
    };
    let journal = journal::read_journal(dir)?;
    let Some(out) = rebuild::rebuild(dir, project, &journal, decoder) else {
        let mut r = RecoveryReport::new(dir, Outcome::Damaged);
        r.notes.push(
            "no project.json and no journal start event: nothing to rebuild, nothing written"
                .to_string(),
        );
        return Ok(r);
    };

    let damaged = out.project.state == ProjectState::Damaged;
    let segments = out.project.recording.screen.segments.len();
    let mut log = SessionLog::open(dir)?;
    log.line(Level::Warn, "recovery: project was left in state recording")?;
    for (level, msg) in &out.log {
        log.line(*level, &format!("recovery: {msg}"))?;
    }
    out.project.save(dir)?;
    let (level, state) = if damaged {
        (Level::Error, "damaged")
    } else {
        (Level::Info, "recovered")
    };
    log.line(
        level,
        &format!("recovery: state {state} with {segments} segment(s)"),
    )?;
    Ok(RecoveryReport {
        dir: dir.to_path_buf(),
        outcome: if damaged {
            Outcome::Damaged
        } else {
            Outcome::Recovered
        },
        segments,
        dropped_segments: out.dropped_segments,
        notes: out.notes,
    })
}

/// Recovers every project folder directly inside `projects_dir` whose `project.json` is in state
/// `recording` (or missing next to a journal), in name order. A missing `projects_dir` yields no
/// reports. A project whose recovery fails with an error is reported as `Damaged` with the error
/// in its notes, is left unchanged, and the scan goes on.
pub fn scan(projects_dir: &Path, decoder: &dyn Decoder) -> Result<Vec<RecoveryReport>> {
    let read_err = |e: std::io::Error| {
        Error::Core(sc_core::Error::Io(std::io::Error::new(
            e.kind(),
            format!("read projects folder {}: {e}", projects_dir.display()),
        )))
    };
    let entries = match fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(read_err(e)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        dirs.push(entry.map_err(read_err)?.path());
    }
    dirs.sort();
    let mut reports = Vec::new();
    for dir in dirs {
        if !folder::is_project_dir(&dir) {
            continue;
        }
        match load(&dir) {
            Loaded::Recording(_) | Loaded::Missing => {}
            Loaded::Final(_) => continue,
            Loaded::Unreadable(e) => {
                tracing::warn!("recovery scan: skipping {}: {e}", dir.display());
                continue;
            }
        }
        match recover_project(&dir, decoder) {
            Ok(r) => reports.push(r),
            Err(e) => {
                tracing::warn!("recovery of {} failed: {e}", dir.display());
                let mut r = RecoveryReport::new(&dir, Outcome::Damaged);
                r.notes
                    .push(format!("recovery failed, project left unchanged: {e}"));
                reports.push(r);
            }
        }
    }
    Ok(reports)
}
