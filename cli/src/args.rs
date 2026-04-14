use anyhow::{anyhow, bail, Result};
use shared::{LayoutMode, ProviderKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Run(RunArgs),
    DumpScreen(DumpScreenArgs),
    ProfileScroll(ProfileScrollArgs),
    Search(SearchArgs),
    Read(ReadArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunArgs {
    pub profile: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchArgs {
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadArgs {
    pub conversation: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileScrollArgs {
    pub workspace: String,
    pub conversation: String,
    pub provider: Option<ProviderKind>,
    pub width: u16,
    pub height: u16,
    pub now_ms: Option<i64>,
    pub steps: usize,
    pub message_index: usize,
    pub direction: ScrollDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DumpScreenArgs {
    pub screen: ScreenTarget,
    pub workspace: Option<String>,
    pub conversation: Option<String>,
    pub provider: Option<ProviderKind>,
    pub layout: Option<LayoutMode>,
    pub width: u16,
    pub height: u16,
    pub now_ms: Option<i64>,
    pub selected: usize,
    pub message_index: usize,
    pub expand_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenTarget {
    Workspaces,
    Conversations,
    History,
    Messages,
}

impl Default for DumpScreenArgs {
    fn default() -> Self {
        Self {
            screen: ScreenTarget::Workspaces,
            workspace: None,
            conversation: None,
            provider: None,
            layout: None,
            width: 120,
            height: 40,
            now_ms: None,
            selected: 0,
            message_index: 0,
            expand_all: false,
        }
    }
}

pub fn parse_args<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let _program = args.next();

    match args.next() {
        None => Ok(Command::Run(RunArgs::default())),
        Some(arg) if arg == "run" => parse_run(args),
        Some(arg) if arg.starts_with("--") => parse_run(std::iter::once(arg).chain(args)),
        Some(arg) if arg == "dump-screen" => parse_dump_screen(args),
        Some(arg) if arg == "profile-scroll" => parse_profile_scroll(args),
        Some(arg) if arg == "search" => parse_search(args),
        Some(arg) if arg == "read" => parse_read(args),
        Some(other) => {
            bail!(
                "unknown command '{other}'. expected 'run', 'dump-screen', 'profile-scroll', 'search', or 'read'"
            )
        }
    }
}

fn parse_run<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut parsed = RunArgs::default();

    for arg in args {
        match arg.as_str() {
            "--profile" => parsed.profile = true,
            other => bail!("unknown run flag '{other}'"),
        }
    }

    Ok(Command::Run(parsed))
}

fn parse_profile_scroll<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut workspace = None;
    let mut conversation = None;
    let mut provider = None;
    let mut width = 120u16;
    let mut height = 40u16;
    let mut now_ms = None;
    let mut steps = 100usize;
    let mut message_index = 0usize;
    let mut direction = ScrollDirection::Down;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => workspace = Some(next_value(&mut args, "--workspace")?),
            "--conversation" => conversation = Some(next_value(&mut args, "--conversation")?),
            "--provider" => {
                let value = next_value(&mut args, "--provider")?;
                provider = Some(parse_provider(&value)?);
            }
            "--width" => {
                let value = next_value(&mut args, "--width")?;
                width = parse_u16(&value, "--width")?;
            }
            "--height" => {
                let value = next_value(&mut args, "--height")?;
                height = parse_u16(&value, "--height")?;
            }
            "--now-ms" => {
                let value = next_value(&mut args, "--now-ms")?;
                now_ms = Some(parse_i64(&value, "--now-ms")?);
            }
            "--steps" => {
                let value = next_value(&mut args, "--steps")?;
                steps = parse_usize(&value, "--steps")?;
            }
            "--message-index" => {
                let value = next_value(&mut args, "--message-index")?;
                message_index = parse_usize(&value, "--message-index")?;
            }
            "--direction" => {
                let value = next_value(&mut args, "--direction")?;
                direction = match value.as_str() {
                    "down" => ScrollDirection::Down,
                    "up" => ScrollDirection::Up,
                    other => bail!("invalid value '{other}' for --direction: expected down or up"),
                };
            }
            other => bail!("unknown profile-scroll flag '{other}'"),
        }
    }

    let workspace =
        workspace.ok_or_else(|| anyhow!("profile-scroll requires --workspace <path>"))?;
    let conversation = conversation
        .ok_or_else(|| anyhow!("profile-scroll requires --conversation <id-or-title>"))?;

    if width == 0 || height == 0 {
        bail!("--width and --height must be greater than zero");
    }

    Ok(Command::ProfileScroll(ProfileScrollArgs {
        workspace,
        conversation,
        provider,
        width,
        height,
        now_ms,
        steps,
        message_index,
        direction,
    }))
}

fn parse_search<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut query = None;
    let mut limit = 10usize;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = next_value(&mut args, "--limit")?;
                limit = parse_usize(&value, "--limit")?;
            }
            other if other.starts_with("--") => bail!("unknown search flag '{other}'"),
            other => {
                if query.is_some() {
                    bail!("search accepts exactly one query string");
                }
                query = Some(other.to_string());
            }
        }
    }

    let query = query.ok_or_else(|| anyhow!("search requires a query string"))?;
    Ok(Command::Search(SearchArgs { query, limit }))
}

fn parse_read<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut conversation = None;
    let mut offset = 0usize;
    let mut limit = 50usize;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--conversation" => {
                conversation = Some(next_value(&mut args, "--conversation")?);
            }
            "--offset" => {
                let value = next_value(&mut args, "--offset")?;
                offset = parse_usize(&value, "--offset")?;
            }
            "--limit" => {
                let value = next_value(&mut args, "--limit")?;
                limit = parse_usize(&value, "--limit")?;
            }
            other => bail!("unknown read flag '{other}'"),
        }
    }

    let conversation = conversation.ok_or_else(|| anyhow!("read requires --conversation <id>"))?;
    Ok(Command::Read(ReadArgs {
        conversation,
        offset,
        limit,
    }))
}

fn parse_dump_screen<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    let mut parsed = DumpScreenArgs::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screen" => {
                let value = next_value(&mut args, "--screen")?;
                parsed.screen = parse_screen(&value)?;
            }
            "--workspace" => {
                parsed.workspace = Some(next_value(&mut args, "--workspace")?);
            }
            "--conversation" => {
                parsed.conversation = Some(next_value(&mut args, "--conversation")?);
            }
            "--provider" => {
                let value = next_value(&mut args, "--provider")?;
                parsed.provider = Some(parse_provider(&value)?);
            }
            "--layout" => {
                let value = next_value(&mut args, "--layout")?;
                parsed.layout = Some(parse_layout(&value)?);
            }
            "--width" => {
                let value = next_value(&mut args, "--width")?;
                parsed.width = parse_u16(&value, "--width")?;
            }
            "--height" => {
                let value = next_value(&mut args, "--height")?;
                parsed.height = parse_u16(&value, "--height")?;
            }
            "--now-ms" => {
                let value = next_value(&mut args, "--now-ms")?;
                parsed.now_ms = Some(parse_i64(&value, "--now-ms")?);
            }
            "--selected" => {
                let value = next_value(&mut args, "--selected")?;
                parsed.selected = parse_usize(&value, "--selected")?;
            }
            "--message-index" => {
                let value = next_value(&mut args, "--message-index")?;
                parsed.message_index = parse_usize(&value, "--message-index")?;
            }
            "--expand-all" => {
                parsed.expand_all = true;
            }
            other => bail!("unknown dump-screen flag '{other}'"),
        }
    }

    validate_dump_screen_args(&parsed)?;
    Ok(Command::DumpScreen(parsed))
}

fn validate_dump_screen_args(args: &DumpScreenArgs) -> Result<()> {
    if args.width == 0 || args.height == 0 {
        bail!("--width and --height must be greater than zero");
    }

    match args.screen {
        ScreenTarget::Workspaces => {
            if args.workspace.is_some() {
                bail!("--workspace is only valid for conversations, history, or messages screens");
            }
            if args.conversation.is_some() {
                bail!(
                    "--conversation is only valid for conversations, history, or messages screens"
                );
            }
            if args.layout.is_some() {
                bail!("--layout is only valid for the conversations screen");
            }
            if args.message_index != 0 {
                bail!("--message-index is only valid for history, split conversations, or messages screens");
            }
            if args.expand_all {
                bail!("--expand-all is only valid for the conversations screen");
            }
        }
        ScreenTarget::Conversations => {
            if args.workspace.is_none() {
                bail!("--workspace is required for the conversations screen");
            }
        }
        ScreenTarget::History => {
            if args.workspace.is_none() {
                bail!("--workspace is required for the history screen");
            }
            if args.conversation.is_none() {
                bail!("--conversation is required for the history screen");
            }
            if args.layout.is_some() {
                bail!("--layout is only valid for the conversations screen");
            }
            if args.selected != 0 {
                bail!("--selected is not used for the history screen");
            }
            if args.expand_all {
                bail!("--expand-all is only valid for the conversations screen");
            }
        }
        ScreenTarget::Messages => {
            if args.workspace.is_none() {
                bail!("--workspace is required for the messages screen");
            }
            if args.conversation.is_none() {
                bail!("--conversation is required for the messages screen");
            }
            if args.layout.is_some() {
                bail!("--layout is only valid for the conversations screen");
            }
            if args.selected != 0 {
                bail!("--selected is not used for the messages screen");
            }
            if args.expand_all {
                bail!("--expand-all is only valid for the conversations screen");
            }
        }
    }

    Ok(())
}

fn next_value<I>(args: &mut I, flag: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| anyhow!("missing value for {flag}"))
}

fn parse_screen(value: &str) -> Result<ScreenTarget> {
    match value {
        "workspaces" => Ok(ScreenTarget::Workspaces),
        "conversations" => Ok(ScreenTarget::Conversations),
        "history" => Ok(ScreenTarget::History),
        "messages" => Ok(ScreenTarget::Messages),
        other => {
            bail!("invalid screen '{other}'. expected one of: workspaces, conversations, history, messages")
        }
    }
}

fn parse_provider(value: &str) -> Result<ProviderKind> {
    match value.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => Ok(ProviderKind::ClaudeCode),
        "codex" => Ok(ProviderKind::Codex),
        other => bail!("invalid provider '{other}'. expected one of: claude-code, codex"),
    }
}

fn parse_layout(value: &str) -> Result<LayoutMode> {
    match value.to_ascii_lowercase().as_str() {
        "table" => Ok(LayoutMode::Table),
        "split" => Ok(LayoutMode::Split),
        other => bail!("invalid layout '{other}'. expected one of: table, split"),
    }
}

fn parse_u16(value: &str, flag: &str) -> Result<u16> {
    value
        .parse()
        .map_err(|_| anyhow!("invalid value '{value}' for {flag}: expected an integer"))
}

fn parse_i64(value: &str, flag: &str) -> Result<i64> {
    value
        .parse()
        .map_err(|_| anyhow!("invalid value '{value}' for {flag}: expected an integer"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| anyhow!("invalid value '{value}' for {flag}: expected an integer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn parse_defaults_to_run() {
        assert_eq!(
            parse_args(args(&["cli"])).unwrap(),
            Command::Run(RunArgs::default())
        );
    }

    #[test]
    fn parse_run_profile_flag() {
        assert_eq!(
            parse_args(args(&["cli", "run", "--profile"])).unwrap(),
            Command::Run(RunArgs { profile: true })
        );
    }

    #[test]
    fn parse_implicit_run_profile_flag() {
        assert_eq!(
            parse_args(args(&["cli", "--profile"])).unwrap(),
            Command::Run(RunArgs { profile: true })
        );
    }

    #[test]
    fn parse_dump_screen_args() {
        let parsed = parse_args(args(&[
            "cli",
            "dump-screen",
            "--screen",
            "conversations",
            "--workspace",
            "~/projects/transcript-browser",
            "--layout",
            "split",
            "--provider",
            "codex",
            "--width",
            "88",
            "--height",
            "20",
            "--selected",
            "2",
            "--message-index",
            "1",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::DumpScreen(DumpScreenArgs {
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/transcript-browser".into()),
                conversation: None,
                provider: Some(ProviderKind::Codex),
                layout: Some(LayoutMode::Split),
                width: 88,
                height: 20,
                now_ms: None,
                selected: 2,
                message_index: 1,
                expand_all: false,
            })
        );
    }

    #[test]
    fn parse_rejects_missing_workspace_for_conversations() {
        let err = parse_args(args(&["cli", "dump-screen", "--screen", "conversations"]))
            .unwrap_err()
            .to_string();

        assert!(err.contains("--workspace is required"));
    }

    #[test]
    fn parse_search_args() {
        let parsed =
            parse_args(args(&["cli", "search", "startup latency", "--limit", "5"])).unwrap();
        assert_eq!(
            parsed,
            Command::Search(SearchArgs {
                query: "startup latency".into(),
                limit: 5,
            })
        );
    }

    #[test]
    fn parse_read_args() {
        let parsed = parse_args(args(&[
            "cli",
            "read",
            "--conversation",
            "claude_code:/tmp/foo:bar",
            "--offset",
            "10",
            "--limit",
            "25",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::Read(ReadArgs {
                conversation: "claude_code:/tmp/foo:bar".into(),
                offset: 10,
                limit: 25,
            })
        );
    }

    #[test]
    fn parse_profile_scroll_args() {
        let parsed = parse_args(args(&[
            "cli",
            "profile-scroll",
            "--workspace",
            "~/projects/transcript-browser",
            "--conversation",
            "dump-screen",
            "--provider",
            "codex",
            "--width",
            "100",
            "--height",
            "24",
            "--steps",
            "25",
            "--message-index",
            "10",
            "--direction",
            "up",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::ProfileScroll(ProfileScrollArgs {
                workspace: "~/projects/transcript-browser".into(),
                conversation: "dump-screen".into(),
                provider: Some(ProviderKind::Codex),
                width: 100,
                height: 24,
                now_ms: None,
                steps: 25,
                message_index: 10,
                direction: ScrollDirection::Up,
            })
        );
    }
}
