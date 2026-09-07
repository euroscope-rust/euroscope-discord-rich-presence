//! The `.drp` chat commands, and the help text describing them.

/// The message box handler EuroScope uses for command help, and the sender
/// name plugins announce themselves under in `.help`.
pub const HELP_HANDLER: &str = "HELP";

/// Every command, with a one-line description. Rendered by `.drp help` and
/// `.help drp`; keep it in sync with the command list in the README.
pub const HELP: &[(&str, &str)] = &[
    (".drp help", "Show this help."),
    (".drp status", "Show what the plugin is currently doing."),
    (".drp start", "Resume sending updates to Discord."),
    (
        ".drp stop",
        "Stop sending updates and disconnect from Discord.",
    ),
    (".drp reload", "Reload the settings file."),
];

/// A command line EuroScope handed us that we recognise as ours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// `.drp reload`
    Reload,
    /// `.drp start`
    Start,
    /// `.drp stop`
    Stop,
    /// `.drp status`
    Status,
    /// `.drp` or `.drp help`
    Help,
    /// `.help drp`
    HelpDrp,
    /// `.help`, which asks every plugin to announce itself.
    HelpIndex,
    /// A `.drp` command we don't know.
    Unknown,
}

impl Command {
    /// The command `command_line` asks for, or `None` when it isn't one of
    /// ours and should be left to EuroScope and the other plugins.
    ///
    /// Matching is case insensitive, and trailing arguments we don't expect
    /// make a `.drp` command [`Unknown`](Self::Unknown) rather than silently
    /// running it.
    pub fn parse(command_line: &str) -> Option<Self> {
        let lowercased = command_line.trim().to_lowercase();
        let mut words = lowercased.split_whitespace();
        match (words.next()?, words.next(), words.next()) {
            (".drp", None | Some("help"), None) => Some(Self::Help),
            (".drp", Some("reload"), None) => Some(Self::Reload),
            (".drp", Some("start"), None) => Some(Self::Start),
            (".drp", Some("stop"), None) => Some(Self::Stop),
            (".drp", Some("status"), None) => Some(Self::Status),
            (".drp", ..) => Some(Self::Unknown),
            (".help", None, None) => Some(Self::HelpIndex),
            (".help", Some("drp"), None) => Some(Self::HelpDrp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn parses_known_commands() {
        assert_eq!(Command::parse(".drp reload"), Some(Command::Reload));
        assert_eq!(Command::parse(".drp start"), Some(Command::Start));
        assert_eq!(Command::parse(".drp stop"), Some(Command::Stop));
        assert_eq!(Command::parse(".drp status"), Some(Command::Status));
        assert_eq!(Command::parse(".drp help"), Some(Command::Help));
        assert_eq!(Command::parse(".drp"), Some(Command::Help));
        assert_eq!(Command::parse(".help"), Some(Command::HelpIndex));
        assert_eq!(Command::parse(".help drp"), Some(Command::HelpDrp));
    }

    #[test]
    fn normalises_case_and_whitespace() {
        assert_eq!(Command::parse("  .DRP   Reload  "), Some(Command::Reload));
        assert_eq!(Command::parse(".Help DRP"), Some(Command::HelpDrp));
    }

    #[test]
    fn rejects_foreign_commands() {
        assert_eq!(Command::parse(""), None);
        assert_eq!(Command::parse(".vatsim reload"), None);
        assert_eq!(Command::parse(".help other"), None);
        assert_eq!(Command::parse(".drpsomething"), None);
    }

    #[test]
    fn unknown_drp_subcommands() {
        assert_eq!(Command::parse(".drp nope"), Some(Command::Unknown));
        assert_eq!(Command::parse(".drp reload now"), Some(Command::Unknown));
    }
}
