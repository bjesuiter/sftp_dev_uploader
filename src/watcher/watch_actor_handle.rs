use super::watch_actor::WatchActor;
use std::{
    path::PathBuf,
    sync::mpsc::{channel as std_channel, Receiver as StdReceiver},
};

pub fn start_watching(
    watch_dir: PathBuf,
    upload_initial: bool,
    ignore_includes: Vec<String>,
    ignore_ends: Vec<String>,
) -> Result<StdReceiver<Vec<PathBuf>>, std::io::Error> {
    let (files_to_upload_tx, files_to_upload_rx) = std_channel();

    // Before creating the watch actor, read the initial files in the directory and send them to the outside world
    if upload_initial {
        // Get all files in the directory (recursively) as Vec<PathBuf>, without dir paths
        let files = walkdir::WalkDir::new(&watch_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let path = e.path();
                let path_str = path.to_string_lossy();

                // iterate through all patterns in ignore_includes and ignore_ends
                // and check if path contains or ends with any of them
                ignore_includes.iter().all(|i| !path_str.contains(i))
                    && ignore_ends.iter().all(|e| !path_str.ends_with(e))
            })
            // bjesuiter: all paths from watch_actor are expected to be absolute, therefore they are canonicalized here
            .filter_map(|e| match e.path().canonicalize() {
                Ok(path) => Some(path),
                Err(_) => {println!("Failed to canonicalize path while collecting paths for --initial-upload: {:?}", e.path()); None},
            })
            .collect::<Vec<PathBuf>>();

        // Send the files to the outside world
        files_to_upload_tx.send(files).unwrap();
    }

    // Create the WatchActor instance
    let actor = WatchActor {
        watch_dir,
        ignore_includes,
        ignore_ends,
        files_to_upload_tx,
    };

    // Spawn the actor thread!
    let thread = std::thread::Builder::new().name("watch_actor_main".to_string());
    if let Err(error) = thread.spawn(|| actor.run_self()) {
        eprintln!("Error spawning the watch actor main thread: {:?}", error);
    };

    // return the files channel receiver to be able to listen to the "files-changed' events emitted by the watcher
    Ok(files_to_upload_rx)
}
