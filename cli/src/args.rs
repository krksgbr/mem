use anyhow::{anyhow, bail, Result};
use clap::{error::ErrorKind, Args as ClapArgs, Parser, Subcommand, ValueEnum};
use shared::{LayoutMode, ProviderKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Run(RunArgs),
    DumpScreen(DumpScreenArgs),
    ProfileScroll(ProfileScrollArgs),
    Workspaces(WorkspacesArgs),
    Latest(LatestArgs),
    Search(SearchArgs),
    Read(ReadArgs),
    Help(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunArgs {
    pub profile: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchArgs {
    pub query: String,
    pub limit: usize,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadArgs {
    pub conversation: String,
    pub offset: usize,
    pub limit: usize,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacesArgs {
    pub provider: Option<ProviderKind>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestArgs {
    pub provider: Option<ProviderKind>,
    pub workspace: Option<String>,
    pub limit: usize,
    pub json: bool,
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
    pub screen_ref: Option<String>,
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
            screen_ref: None,
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

#[derive(Parser, Debug)]
#[command(
    name = "mem",
    bin_name = "mem",
    about = "Browse and inspect indexed AI conversations.",
    long_about = None
)]
struct Cli {
    #[arg(
        long,
        help = "Write an interactive profile report during the TUI session.",
        help_heading = "Run Options"
    )]
    profile: bool,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    #[command(about = "Run the interactive terminal UI.")]
    Run(RunCli),

    #[command(about = "List indexed workspaces.")]
    Workspaces(WorkspacesCli),

    #[command(about = "List the most recently active conversations.")]
    Latest(LatestCli),

    #[command(about = "Search indexed conversations.")]
    Search(SearchCli),

    #[command(about = "Read transcript entries from one conversation.")]
    Read(ReadCli),

    #[command(hide = true)]
    DumpScreen(DumpScreenCli),

    #[command(hide = true)]
    ProfileScroll(ProfileScrollCli),
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq, Default)]
struct RunCli {
    #[arg(long, help = "Write an interactive profile report during the TUI session.")]
    profile: bool,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
struct WorkspacesCli {
    #[arg(long, value_enum, help = "Only show workspaces containing conversations from this provider.")]
    provider: Option<ProviderArg>,

    #[arg(long, help = "Emit machine-readable JSON instead of text output.")]
    json: bool,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
struct LatestCli {
    #[arg(long, value_enum, help = "Only show conversations from this provider.")]
    provider: Option<ProviderArg>,

    #[arg(long, help = "Restrict results to one workspace id, display name, or canonical path.")]
    workspace: Option<String>,

    #[arg(long, default_value_t = 10, help = "Maximum number of conversations to show.")]
    limit: usize,

    #[arg(long, help = "Emit machine-readable JSON instead of text output.")]
    json: bool,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
struct SearchCli {
    #[arg(help = "Search query to run against indexed conversations.")]
    query: String,

    #[arg(long, default_value_t = 10, help = "Maximum number of matching conversations to show.")]
    limit: usize,

    #[arg(long, help = "Emit machine-readable JSON instead of text output.")]
    json: bool,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
struct ReadCli {
    #[arg(help = "Conversation selector: internal id, provider external id, or exact title.")]
    selector: Option<String>,

    #[arg(long, help = "Conversation selector: internal id, provider external id, or exact title.")]
    conversation: Option<String>,

    #[arg(long, default_value_t = 0, help = "Zero-based entry offset to start reading from.")]
    offset: usize,

    #[arg(long, default_value_t = 50, help = "Maximum number of entries to return.")]
    limit: usize,

    #[arg(long, help = "Emit machine-readable JSON instead of text output.")]
    json: bool,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
struct ProfileScrollCli {
    #[arg(long)]
    workspace: String,
    #[arg(long)]
    conversation: String,
    #[arg(long, value_enum)]
    provider: Option<ProviderArg>,
    #[arg(long, default_value_t = 120)]
    width: u16,
    #[arg(long, default_value_t = 40)]
    height: u16,
    #[arg(long)]
    now_ms: Option<i64>,
    #[arg(long, default_value_t = 100)]
    steps: usize,
    #[arg(long, default_value_t = 0)]
    message_index: usize,
    #[arg(long, value_enum, default_value_t = DirectionArg::Down)]
    direction: DirectionArg,
}

#[derive(ClapArgs, Debug, Clone, PartialEq, Eq, Default)]
struct DumpScreenCli {
    #[arg(long)]
    screen_ref: Option<String>,
    #[arg(long, value_enum, default_value_t = ScreenArg::Workspaces)]
    screen: ScreenArg,
    #[arg(long)]
    workspace: Option<String>,
    #[arg(long)]
    conversation: Option<String>,
    #[arg(long, value_enum)]
    provider: Option<ProviderArg>,
    #[arg(long, value_enum)]
    layout: Option<LayoutArg>,
    #[arg(long, default_value_t = 120)]
    width: u16,
    #[arg(long, default_value_t = 40)]
    height: u16,
    #[arg(long)]
    now_ms: Option<i64>,
    #[arg(long, default_value_t = 0)]
    selected: usize,
    #[arg(long, default_value_t = 0)]
    message_index: usize,
    #[arg(long, default_value_t = false)]
    expand_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProviderArg {
    #[value(alias = "claude")]
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LayoutArg {
    Table,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum DirectionArg {
    #[default]
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum ScreenArg {
    #[default]
    Workspaces,
    Conversations,
    History,
    Messages,
}

impl From<ProviderArg> for ProviderKind {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::ClaudeCode => ProviderKind::ClaudeCode,
            ProviderArg::Codex => ProviderKind::Codex,
        }
    }
}

impl From<LayoutArg> for LayoutMode {
    fn from(value: LayoutArg) -> Self {
        match value {
            LayoutArg::Table => LayoutMode::Table,
            LayoutArg::Split => LayoutMode::Split,
        }
    }
}

impl From<DirectionArg> for ScrollDirection {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Down => ScrollDirection::Down,
            DirectionArg::Up => ScrollDirection::Up,
        }
    }
}

impl From<ScreenArg> for ScreenTarget {
    fn from(value: ScreenArg) -> Self {
        match value {
            ScreenArg::Workspaces => ScreenTarget::Workspaces,
            ScreenArg::Conversations => ScreenTarget::Conversations,
            ScreenArg::History => ScreenTarget::History,
            ScreenArg::Messages => ScreenTarget::Messages,
        }
    }
}

pub fn parse_args<I>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = String>,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => convert_cli(cli),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Ok(Command::Help(error.to_string())),
            _ => Err(anyhow!(error.to_string())),
        },
    }
}

fn convert_cli(cli: Cli) -> Result<Command> {
    match cli.command {
        None => Ok(Command::Run(RunArgs {
            profile: cli.profile,
        })),
        Some(CliCommand::Run(run)) => Ok(Command::Run(RunArgs {
            profile: cli.profile || run.profile,
        })),
        Some(CliCommand::Workspaces(args)) => Ok(Command::Workspaces(WorkspacesArgs {
            provider: args.provider.map(Into::into),
            json: args.json,
        })),
        Some(CliCommand::Latest(args)) => Ok(Command::Latest(LatestArgs {
            provider: args.provider.map(Into::into),
            workspace: args.workspace,
            limit: args.limit,
            json: args.json,
        })),
        Some(CliCommand::Search(args)) => Ok(Command::Search(SearchArgs {
            query: args.query,
            limit: args.limit,
            json: args.json,
        })),
        Some(CliCommand::Read(args)) => {
            let conversation = resolve_read_selector(args.selector, args.conversation)?;
            Ok(Command::Read(ReadArgs {
                conversation,
                offset: args.offset,
                limit: args.limit,
                json: args.json,
            }))
        }
        Some(CliCommand::DumpScreen(args)) => {
            let parsed = DumpScreenArgs {
                screen_ref: args.screen_ref,
                screen: args.screen.into(),
                workspace: args.workspace,
                conversation: args.conversation,
                provider: args.provider.map(Into::into),
                layout: args.layout.map(Into::into),
                width: args.width,
                height: args.height,
                now_ms: args.now_ms,
                selected: args.selected,
                message_index: args.message_index,
                expand_all: args.expand_all,
            };
            validate_dump_screen_args(&parsed)?;
            Ok(Command::DumpScreen(parsed))
        }
        Some(CliCommand::ProfileScroll(args)) => {
            if args.width == 0 || args.height == 0 {
                bail!("--width and --height must be greater than zero");
            }
            Ok(Command::ProfileScroll(ProfileScrollArgs {
                workspace: args.workspace,
                conversation: args.conversation,
                provider: args.provider.map(Into::into),
                width: args.width,
                height: args.height,
                now_ms: args.now_ms,
                steps: args.steps,
                message_index: args.message_index,
                direction: args.direction.into(),
            }))
        }
    }
}

fn resolve_read_selector(
    selector: Option<String>,
    conversation_flag: Option<String>,
) -> Result<String> {
    match (selector, conversation_flag) {
        (Some(_), Some(_)) => bail!("read accepts exactly one conversation selector"),
        (Some(selector), None) => Ok(selector),
        (None, Some(selector)) => Ok(selector),
        (None, None) => bail!("read requires a conversation selector"),
    }
}

fn validate_dump_screen_args(args: &DumpScreenArgs) -> Result<()> {
    if args.width == 0 || args.height == 0 {
        bail!("--width and --height must be greater than zero");
    }

    if args.screen_ref.is_some() {
        if args.workspace.is_some() {
            bail!("--workspace cannot be combined with --screen-ref");
        }
        if args.conversation.is_some() {
            bail!("--conversation cannot be combined with --screen-ref");
        }
        if args.provider.is_some() {
            bail!("--provider cannot be combined with --screen-ref");
        }
        if args.layout.is_some() {
            bail!("--layout cannot be combined with --screen-ref");
        }
        if args.selected != 0 {
            bail!("--selected cannot be combined with --screen-ref");
        }
        if args.message_index != 0 {
            bail!("--message-index cannot be combined with --screen-ref");
        }
        if args.expand_all {
            bail!("--expand-all cannot be combined with --screen-ref");
        }
        return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn parse_defaults_to_run() {
        assert_eq!(
            parse_args(args(&["mem"])).unwrap(),
            Command::Run(RunArgs::default())
        );
    }

    #[test]
    fn parse_run_profile_flag() {
        assert_eq!(
            parse_args(args(&["mem", "run", "--profile"])).unwrap(),
            Command::Run(RunArgs { profile: true })
        );
    }

    #[test]
    fn parse_implicit_run_profile_flag() {
        assert_eq!(
            parse_args(args(&["mem", "--profile"])).unwrap(),
            Command::Run(RunArgs { profile: true })
        );
    }

    #[test]
    fn parse_dump_screen_args() {
        let parsed = parse_args(args(&[
            "mem",
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
                screen_ref: None,
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
        let err = parse_args(args(&["mem", "dump-screen", "--screen", "conversations"]))
            .unwrap_err()
            .to_string();

        assert!(err.contains("--workspace is required"));
    }

    #[test]
    fn parse_search_args() {
        let parsed =
            parse_args(args(&["mem", "search", "startup latency", "--limit", "5"])).unwrap();
        assert_eq!(
            parsed,
            Command::Search(SearchArgs {
                query: "startup latency".into(),
                limit: 5,
                json: false,
            })
        );
    }

    #[test]
    fn parse_search_json_flag() {
        let parsed = parse_args(args(&["mem", "search", "lightdash", "--json"])).unwrap();
        assert_eq!(
            parsed,
            Command::Search(SearchArgs {
                query: "lightdash".into(),
                limit: 10,
                json: true,
            })
        );
    }

    #[test]
    fn parse_read_args() {
        let parsed = parse_args(args(&[
            "mem",
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
                json: false,
            })
        );
    }

    #[test]
    fn parse_read_accepts_positional_selector() {
        let parsed = parse_args(args(&[
            "mem",
            "read",
            "36b83837-732d-4796-9def-6eea6652f267",
            "--limit",
            "20",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::Read(ReadArgs {
                conversation: "36b83837-732d-4796-9def-6eea6652f267".into(),
                offset: 0,
                limit: 20,
                json: false,
            })
        );
    }

    #[test]
    fn parse_workspaces_args() {
        let parsed =
            parse_args(args(&["mem", "workspaces", "--provider", "claude-code", "--json"]))
                .unwrap();

        assert_eq!(
            parsed,
            Command::Workspaces(WorkspacesArgs {
                provider: Some(ProviderKind::ClaudeCode),
                json: true,
            })
        );
    }

    #[test]
    fn parse_latest_args() {
        let parsed = parse_args(args(&[
            "mem",
            "latest",
            "--provider",
            "codex",
            "--workspace",
            "~/unbody/bookmarking",
            "--limit",
            "3",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::Latest(LatestArgs {
                provider: Some(ProviderKind::Codex),
                workspace: Some("~/unbody/bookmarking".into()),
                limit: 3,
                json: false,
            })
        );
    }

    #[test]
    fn parse_read_help_flag_returns_help_command() {
        let parsed = parse_args(args(&["mem", "read", "--help"])).unwrap();
        match parsed {
            Command::Help(text) => {
                assert!(text.contains("Conversation selector"));
                assert!(text.contains("--json"));
            }
            other => panic!("expected help command, got {other:?}"),
        }
    }

    #[test]
    fn parse_top_level_help_flag_returns_help_command() {
        let parsed = parse_args(args(&["mem", "--help"])).unwrap();
        match parsed {
            Command::Help(text) => {
                assert!(text.contains("Browse and inspect indexed AI conversations."));
                assert!(text.contains("workspaces"));
                assert!(text.contains("latest"));
            }
            other => panic!("expected help command, got {other:?}"),
        }
    }

    #[test]
    fn parse_profile_scroll_args() {
        let parsed = parse_args(args(&[
            "mem",
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

    #[test]
    fn parse_dump_screen_screen_ref_args() {
        let parsed = parse_args(args(&[
            "mem",
            "dump-screen",
            "--screen-ref",
            "./transcript-browser-screen-ref.json",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Command::DumpScreen(DumpScreenArgs {
                screen_ref: Some("./transcript-browser-screen-ref.json".into()),
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
            })
        );
    }
}
