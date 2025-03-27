use std::path::PathBuf;

pub mod upload_pair;

// use clap::builder::NumberParser;
use clap::{
    crate_authors, crate_description, crate_version, value_parser, Arg, ArgAction, Command,
};

pub fn setup_cli() -> Command {
    Command::new("dev-uploader")
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(Arg::new("upload_pair")
                .short('u')
                .long("upload-pair")
                .value_name("upload-pair")
                // TODO: Allow ArgAction::Append to allow multiple upload pairs
                // Read it in main.rs like this:
                // let upload_pairs = matches
                // .get_many::<String>("upload_pair")
                // .unwrap_or_default()
                // .map(|v| v.as_str())
                // .collect::<Vec<_>>();
                .action(ArgAction::Set)
                .help(
                    [
                        "An upload-pair in the format of <source>[:target].",
                        "The source folder will be watched and changes will be uploaded to the target path on the destination host.",
                        "- If only <source> is provided, the destination will be the same as the source ", 
                        "          (only works with relative paths!).",
                        "- If the content of source should be uploaded to the cwd of the remote host, use '.' or './' as target.",
                        "  Example: '-u playground:.' will upload the content of the playground folder to the cwd of the remote host.",
                        "- Currently only one upload-pair is supported and required.",
                        "- Later: support for multiple upload-pairs should be added.",
                    ]
                    .join("\n"),
                )
                .required(true),
        )
        .arg(Arg::new("host")
                .short('H') // small h already used for help
                .long("host")
                .value_name("sftp-host")
                .help([
                    "The sftp host to connect to.", 
                    "Should be a valid hostname or IP address.",
                ].join("\n"))
                .required(true)
        )
        .arg(Arg::new("port")
                .short('P')
                .long("port")
                .value_name("sftp-port")
                .help([
                    "The sftp port to connect to.", 
                ].join("\n"))
                .value_parser(value_parser!(u16))
                .required(false)
                .default_value("22")
        )
        .arg(Arg::new("username")
                .short('U') // small u already used for upload-pair
                .long("username")
                .value_name("sftp-username")
                .help([
                    "The sftp username to use for the connection.", 
                ].join("\n"))
                .required(true)
        )
        .arg(Arg::new("pubkey")
                .short('k')
                .long("pubkey")
                .value_name("public-key-path")
                .help([
                    "The path to the public key file to use for the sftp connection.", 
                    "Should be a valid path to a public key file.",
                ].join("\n"))
                .value_parser(value_parser!(PathBuf))
                .conflicts_with("password")
                .required(true)
        )
        .arg(Arg::new("privkey")
                .short('K')
                .long("privkey")
                .value_name("private-key-path")
                .help([
                    "The path to the private key file to use for the sftp connection.", 
                    "Should be a valid path to a private key file.",
                ].join("\n"))
                .value_parser(value_parser!(PathBuf))
                .conflicts_with("password")
                .required(true)
        )
        .arg(Arg::new("passphrase")
                .short('S')
                .long("passphrase")
                .value_name("passphrase")
                .help([
                    "The passphrase to use for the sftp connection when using pubkey auth.", 
                ].join("\n"))
                .conflicts_with("password")
                .required(false)
        )
        .arg(Arg::new("password")
            .short('W') // Big P is already used for port
            .long("password")
            .value_name("sftp-password")
            .help([
                "The sftp password to use for the connection.", 
                "Should be a valid password.",
            ].join("\n"))
            .required(true)
            .conflicts_with_all(&["pubkey", "privkey"])
        )
        .arg(
            Arg::new("connection_count")
                .short('c')
                .long("connections")
                .required(false)
                .value_name("connection-count")
                .value_parser(value_parser!(u8))
                .help("Number of connections to use for the sftp upload.")
                .default_value("6")
        )
        .arg(
            Arg::new("watcher_ignore_path_includes")
                .short('i')
                .long("ignore-path-includes")
                .value_name("includes_pattern")
                .action(ArgAction::Append)
                .help([
                    "Optional: Path patterns to ignore in the watcher. ",
                    "Will be checked via string.includes().",
                    "Can be added multiple times.",
                    "For example: '-i .js.map -i stats.js' will filter all file paths containing '.js.map' or 'stats.js'.",
                    "Note: prefer --ignore-path-ends if possible."
                ].join("\n"))
        )
        .arg(
            Arg::new("watcher_ignore_path_ends_with")
                .short('e')
                .long("ignore-path-ends")
                .value_name("ends_pattern")
                .action(ArgAction::Append)
                .help([
                    "Optional: Path patterns to ignore in the watcher. ",
                    "Will be checked via string.ends-with().",
                    "Can be added multiple times.",
                    "For example: '-e .js.map -e stats.js' will filter all file paths ending with '.js.map' or 'stats.js'.",
                ].join("\n"))
        )
        .arg(
            Arg::new("upload_initial")
                .short('I')
                .long("upload-initial")
                .help("Upload all files from the source to the target path on the destination host before starting the watcher.")
                .action(clap::ArgAction::SetTrue)
                .default_value("false")
        )
}
