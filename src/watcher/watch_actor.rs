use std::collections::HashSet;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;
use std::sync::mpsc::Sender as StdSender;
use std::time::Duration;

use miette::IntoDiagnostic;
use tokio::io::Result as TokioResult;
use watchexec::Watchexec;
use watchexec_events::filekind::{FileEventKind, ModifyKind};
use watchexec_events::Tag;
use watchexec_signals::Signal;

/**
 * A watch actor: It will watch a directory for changes and send the paths of changed files to the outside world
 */
pub struct WatchActor {
    // Inner state for the actor:
    pub watch_dir: PathBuf,
    pub ignore_includes: Vec<String>,
    pub ignore_ends: Vec<String>,
    /**
     * The watch_event_tx is a Sender which will be used to send the paths of changed files to the outside world
     */
    pub files_to_upload_tx: StdSender<Vec<PathBuf>>,
}

impl WatchActor {
    /**
     * Runs the main loop of the actor, aka.
     * - watches a dir for changes and sends files_to_upload back through a channel
     * Important: This function must own the actor (no `&mut self``, but only `mut self``)
     */
    pub fn run_self(mut self) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            // This blocks until the watcher is closed
            let _ = self.watch().await;
            ()
        })
    }

    pub async fn watch(&mut self) -> TokioResult<()> {
        let files_to_upload_tx = self.files_to_upload_tx.clone();
        let watch_dir = self.watch_dir.clone();
        let ignore_includes = self.ignore_includes.clone();
        let ignore_ends = self.ignore_ends.clone();

        let wx = Watchexec::new(move |mut action| {
            // Debug print
            // eprintln!(
            //     "{} Detected file changes, pre filter: {}",
            //     Utc::now(),
            //     action.events.len()
            // );

            // if Ctrl-C is received, quit
            if action.signals().any(|sig| sig == Signal::Interrupt) {
                action.quit();
            }

            // filter the original events to the one i'm interested in!
            let events_iter = action.events.iter().filter_map(|event| {
                // pure event log, if needed
                // eprintln!("EVENT: {event:?}\n");

                // Iterate over the tags of an event to decide if it should be filtered or not
                let some_event_or_none =
                    match_event_by_tags(&event.tags, &ignore_includes, &ignore_ends);
                some_event_or_none
            });

            let files_to_upload = HashSet::<PathBuf>::from_iter(events_iter.cloned());

            match files_to_upload_tx.send(files_to_upload.into_iter().collect()) {
                Ok(_) => (),
                Err(e) => eprintln!("Error sending files to upload: {:?}", e),
            }

            action
        });

        let ensure_wx = match wx {
            Ok(w) => w,
            Err(e) => {
                eprintln!("CriticalError creating Watchexec instance: {:?}", e);
                return Err(Error::new(
                    ErrorKind::Other,
                    "CriticalError creating Watchexec instance",
                ));
            }
        };

        // watch the path sent to this function
        ensure_wx.config.pathset([watch_dir]);
        ensure_wx.config.throttle(Duration::from_millis(1500));

        match ensure_wx.main().await.into_diagnostic() {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Error after Watchexec main loop: {:?}", e);
                return Err(Error::new(
                    ErrorKind::Other,
                    "Error after Watchexec main loop",
                ));
            }
        };
    }
}

// match tags as per event types: https://docs.rs/watchexec-events/latest/watchexec_events/
fn match_event_by_tags<'a>(
    tags: &'a Vec<Tag>,
    ignore_includes: &'a Vec<String>,
    ignore_ends: &'a Vec<String>,
) -> Option<&'a PathBuf> {
    let mut result_path = None;

    for tag in tags {
        match tag {
            Tag::Path { path, file_type } => {
                // println!("Path: {:?}, File Type: {:?}", path, file_type.unwrap_or(FileType::Other));
                // TODO: implement correct ignore logic

                // Step 1: ignore directories
                // They will be implicitly handled by the sftp uploader, like git is doing it
                if path.is_dir() {
                    return None;
                }

                // Step 2: ignore files based on how their paths ends
                for pattern in ignore_ends {
                    // convert to string first, since path.ends_with() only works with full path segments!
                    if path.to_str().unwrap().ends_with(pattern) {
                        return None;
                    }
                }

                // Step 3: ignore files based on how their paths include a certain string
                for pattern in ignore_includes {
                    if path.to_str().unwrap().contains(pattern) {
                        return None;
                    }
                }

                result_path = Some(path);
            }
            Tag::FileEventKind(kind) => {
                // println!("    File Event Kind: {:?}", kind);
                match kind {
                    FileEventKind::Any => {
                        // println!("File Event: Any");
                    }
                    FileEventKind::Access(_access_kind) => {
                        // println!("File Event: Access, Kind: {:?}", access_kind);
                        return None;
                    }
                    FileEventKind::Create(_create_kind) => {
                        // println!("File Event: Create, Kind: {:?}", create_kind);
                    }
                    FileEventKind::Modify(modify_kind) => {
                        // println!("File Event: Modify, Kind: {:?}", modify_kind);

                        // remove all events of Kind: Metadata(Any)
                        match modify_kind {
                            ModifyKind::Any => {
                                // println!("    It's a general modification event.");
                            }
                            ModifyKind::Data(_data_change) => {
                                // println!("    Data content changed.");
                            }
                            ModifyKind::Metadata(_metadata_kind) => {
                                // println!("  => Removed in filter, Reason: is Modify Event for Metadata ");
                                return None;
                            }
                            ModifyKind::Name(_rename_mode) => {
                                // println!("    File or folder name changed.");
                            }
                            ModifyKind::Other => {
                                // println!("    It's a different kind of modification event.");
                            }
                        }
                    }
                    FileEventKind::Remove(_remove_kind) => {
                        // println!("File Event: Remove, Kind: {:?}", remove_kind);
                        return None;
                    }
                    FileEventKind::Other => {
                        // println!("File Event: Other");
                        return None;
                    }
                }
            }
            Tag::Source(_source) => {
                // println!("Source: {:?}", source);
                // DO NOT return 'None' from here, because the filesystem events all have a Source(Filesystem) tag!
            }
            Tag::Process(pid) => {
                // should not occur with angular build, leaving the print on to be able to see it!
                println!("Process: {:?}", pid);
            }
            Tag::ProcessCompletion(completion) => {
                println!("Process Completion: {:?}", completion);
            }
            Tag::Keyboard(_keyboard) => {
                // occurs when someone types into the terminal, for example for STRG+C
                // println!("Keyboard: {:?}", keyboard);
                return None;
            }
            Tag::Signal(_signal) => {
                // occurs when cli sends some signal, like STRG+C
                // println!("Signal: {:?}", signal);
                return None;
            }
            _ => println!("Unknown: {:?}", tag),
        }
    }

    return result_path;
}
