mod avcap;
mod capture;
mod decode;
mod encbench;
mod record;
mod swap;
mod testwin;
mod util;
mod writer;

use std::collections::HashMap;

pub struct Args {
    kv: HashMap<String, String>,
    flags: Vec<String>,
    pub pos: Vec<String>,
}

impl Args {
    fn parse(v: &[String]) -> Self {
        let mut kv = HashMap::new();
        let mut flags = vec![];
        let mut pos = vec![];
        let mut i = 0;
        while i < v.len() {
            if let Some(k) = v[i].strip_prefix("--") {
                if i + 1 < v.len() && !v[i + 1].starts_with("--") {
                    kv.insert(k.to_string(), v[i + 1].clone());
                    i += 2;
                    continue;
                }
                flags.push(k.to_string());
            } else {
                pos.push(v[i].clone());
            }
            i += 1;
        }
        Self { kv, flags, pos }
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.kv.get(k).map(|s| s.as_str())
    }
    pub fn num(&self, k: &str, d: f64) -> f64 {
        self.get(k).and_then(|s| s.parse().ok()).unwrap_or(d)
    }
    pub fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }
}

const USAGE: &str = "sc-spike <list|record|activity|killtest> [--key value] [--flag]
  list                          enumerate displays/windows/apps, cameras, mics, permission state
  record --out DIR [--secs 10] [--fps 30] [--pixfmt 420v|bgra] [--queue-depth 6] [--exclude windows|app|none]
         [--codec hevc|h264] [--bpp 0.05] [--frag 1.0] [--camera NAME] [--no-camera] [--no-mic] [--no-sysaudio]
         [--no-test-window] [--no-activity]
  killtest --out DIR [--after 6]  record in a child process and SIGKILL it after N seconds
  swap --out DIR [--strategy update|restart|overlap] [--audio shared|separate] [--order filter-first|config-first]
       [--secs 12] [--interval 3]   live source switch display -> window -> region -> display, one segment file per source
  picker                        configure SCContentSharingPicker without presenting it";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let a = Args::parse(&argv[1..]);
    let r = match cmd.as_str() {
        "list" => record::list(&a),
        "perms" => record::perms(&a),
        "record" => record::record(&a),
        "activity" => record::activity(&a),
        "killtest" => record::killtest(&a),
        "decode" => decode::decode(&a),
        "encbench" => encbench::encbench(&a),
        "swap" => swap::swap(&a),
        "swaphelper" => swap::swaphelper(&a),
        "picker" => swap::picker(&a),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
