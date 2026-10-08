//! Command-line interface: `wallmotion-settings --set file.mp4 --stop --mute`.
//!
//! Port of `wallmotion/cli.py`: scriptable control of the app. When another
//! instance is already running, the command is forwarded to it (see
//! `remote`); otherwise it applies to the freshly started instance.

/// Parsed CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    pub set: Option<String>,
    pub stop: bool,
    pub muted: Option<bool>,
    pub volume: Option<u8>,
    pub version: bool,
}

pub fn usage() -> &'static str {
    "wallmotion-settings [--set FILE] [--stop] [--mute|--unmute] [--volume 0-100] [--version]"
}

/// Parse CLI args (without the program name). Err = usage error message.
/// Mirrors `parse_args()`: `--mute` + `--unmute` conflict, `--volume` needs
/// an integer, unknown flags are rejected.
pub fn parse_args(argv: &[String]) -> Result<CliArgs, String> {
    let mut out = CliArgs::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--set" => {
                i += 1;
                let val = argv.get(i).ok_or("--set needs a FILE value")?;
                out.set = Some(val.clone());
            }
            "--stop" => out.stop = true,
            "--mute" => {
                if out.muted == Some(false) {
                    return Err("--mute and --unmute are exclusive".to_string());
                }
                out.muted = Some(true);
            }
            "--unmute" => {
                if out.muted == Some(true) {
                    return Err("--mute and --unmute are exclusive".to_string());
                }
                out.muted = Some(false);
            }
            "--volume" => {
                i += 1;
                let raw = argv.get(i).ok_or("--volume needs a 0-100 value")?;
                let n: i64 = raw
                    .parse()
                    .map_err(|_| format!("--volume needs an integer, got {raw:?}"))?;
                out.volume = Some(n.clamp(0, 100) as u8);
            }
            "--version" => out.version = true,
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

/// Convert parsed args to a remote-command object (JSON-serializable).
/// Mirrors `args_to_command()`: only actionable keys are included.
pub fn args_to_command(args: &CliArgs) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(set) = &args.set {
        map.insert("set".to_string(), serde_json::Value::String(set.clone()));
    }
    if args.stop {
        map.insert("stop".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(muted) = args.muted {
        map.insert("muted".to_string(), serde_json::Value::Bool(muted));
    }
    if let Some(volume) = args.volume {
        map.insert(
            "volume".to_string(),
            serde_json::Value::Number(volume.into()),
        );
    }
    serde_json::Value::Object(map)
}

/// True when any actionable flag was given (ignores `--version`).
/// Mirrors `has_action()`.
pub fn has_action(args: &CliArgs) -> bool {
    !args_to_command(args)
        .as_object()
        .map(|m| m.is_empty())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty() {
        let args = parse_args(&[]).unwrap();
        assert_eq!(args.set, None);
        assert!(!args.stop);
        assert_eq!(args.volume, None);
        assert_eq!(args_to_command(&args), serde_json::json!({}));
        assert!(!has_action(&args));
    }

    #[test]
    fn set() {
        let args = parse_args(&argv(&["--set", "a.mp4"])).unwrap();
        assert_eq!(args.set.as_deref(), Some("a.mp4"));
        assert_eq!(args_to_command(&args), serde_json::json!({"set": "a.mp4"}));
        assert!(has_action(&args));
    }

    #[test]
    fn stop_mute_volume() {
        let args = parse_args(&argv(&["--stop", "--mute", "--volume", "40"])).unwrap();
        assert_eq!(
            args_to_command(&args),
            serde_json::json!({"stop": true, "muted": true, "volume": 40})
        );
    }

    #[test]
    fn unmute() {
        let args = parse_args(&argv(&["--unmute"])).unwrap();
        assert_eq!(args_to_command(&args), serde_json::json!({"muted": false}));
    }

    #[test]
    fn mute_unmute_exclusive() {
        assert!(parse_args(&argv(&["--mute", "--unmute"])).is_err());
    }

    #[test]
    fn volume_clamped() {
        let args = parse_args(&argv(&["--volume", "999"])).unwrap();
        assert_eq!(args_to_command(&args)["volume"], serde_json::json!(100));
        let args = parse_args(&argv(&["--volume", "-5"])).unwrap();
        assert_eq!(args_to_command(&args)["volume"], serde_json::json!(0));
    }

    #[test]
    fn version_flag() {
        assert!(parse_args(&argv(&["--version"])).unwrap().version);
    }
}
