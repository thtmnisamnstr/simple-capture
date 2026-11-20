//! App state machine: Idle → Recording ⇄ Switching → Stopping → Saved → Exporting.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Recording,
    Switching,
    Stopping,
    Saved { project_dir: PathBuf },
    Exporting { project_dir: PathBuf, job: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Start,
    SwitchBegin,
    SwitchEnd,
    StopBegin,
    Finalized { project_dir: PathBuf },
    Failed,
    ExportStart { project_dir: PathBuf, job: String },
    ExportEnd,
}

impl Phase {
    pub fn is_recording(&self) -> bool {
        matches!(self, Phase::Recording | Phase::Switching | Phase::Stopping)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Recording => "recording",
            Phase::Switching => "switching",
            Phase::Stopping => "stopping",
            Phase::Saved { .. } => "saved",
            Phase::Exporting { .. } => "exporting",
        }
    }

    /// The next phase, or why `input` is not allowed now.
    pub fn next(&self, input: Input) -> Result<Phase, String> {
        use Input as I;
        use Phase as P;
        let next = match (self, input) {
            (P::Idle | P::Saved { .. }, I::Start) => P::Recording,
            (P::Recording, I::SwitchBegin) => P::Switching,
            (P::Switching | P::Recording, I::SwitchEnd) => P::Recording,
            (P::Recording | P::Switching, I::StopBegin) => P::Stopping,
            (P::Recording | P::Switching | P::Stopping, I::Finalized { project_dir }) => {
                P::Saved { project_dir }
            }
            (P::Recording | P::Switching | P::Stopping, I::Failed) => P::Idle,
            (P::Idle | P::Saved { .. }, I::ExportStart { project_dir, job }) => {
                P::Exporting { project_dir, job }
            }
            (P::Exporting { project_dir, .. }, I::ExportEnd) => P::Saved {
                project_dir: project_dir.clone(),
            },
            (p, i) => return Err(format!("{i:?} is not allowed while {}", p.name())),
        };
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        PathBuf::from("/p/x.sc")
    }

    #[test]
    fn full_cycle() {
        let mut p = Phase::Idle;
        for i in [
            Input::Start,
            Input::SwitchBegin,
            Input::SwitchEnd,
            Input::SwitchBegin,
            Input::StopBegin,
            Input::Finalized { project_dir: dir() },
            Input::ExportStart {
                project_dir: dir(),
                job: "j".into(),
            },
            Input::ExportEnd,
            Input::Start,
            Input::Finalized { project_dir: dir() },
        ] {
            p = p.next(i.clone()).unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(p, Phase::Saved { project_dir: dir() });
    }

    #[test]
    fn rejects_out_of_order() {
        assert!(Phase::Idle.next(Input::StopBegin).is_err());
        assert!(Phase::Idle.next(Input::SwitchBegin).is_err());
        assert!(Phase::Recording.next(Input::Start).is_err());
        assert!(Phase::Stopping.next(Input::SwitchBegin).is_err());
        assert!(
            Phase::Recording
                .next(Input::ExportStart {
                    project_dir: dir(),
                    job: "j".into()
                })
                .is_err()
        );
        let exporting = Phase::Exporting {
            project_dir: dir(),
            job: "j".into(),
        };
        assert!(exporting.next(Input::Start).is_err());
        assert_eq!(
            exporting.next(Input::ExportEnd),
            Ok(Phase::Saved { project_dir: dir() })
        );
    }

    #[test]
    fn failure_returns_to_idle() {
        for p in [Phase::Recording, Phase::Switching, Phase::Stopping] {
            assert_eq!(p.next(Input::Failed), Ok(Phase::Idle));
            assert!(p.is_recording());
        }
        assert!(!Phase::Saved { project_dir: dir() }.is_recording());
    }
}
