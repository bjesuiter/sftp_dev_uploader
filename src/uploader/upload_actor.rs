use super::progress_actor_handle::ProgressActorHandle;
use crate::{sftp::sftp_client::SftpClient, utils::split_to_n_chunks};
use chrono::Local;
use core::sync;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{mpsc::Receiver as StdReceiver, Arc, Mutex},
};

pub struct UploadActor {
    // Meta for actor
    pub msg_rx: StdReceiver<UploadActorMessage>,

    // Static info - set on creation
    connection_count: u8,

    // Level 1 - work in main thead of the actor
    // ---------------------------------------------------
    client_names: Vec<String>,

    // Level 2 - work inside child threads (per connection)
    // ---------------------------------------------------
    connections: Vec<Arc<Mutex<SftpClient>>>,
    // This handle is cloneable, so that multiple threads can access it
    progress_handler: ProgressActorHandle,
}

pub enum UploadActorMessage {
    UploadFiles {
        files: Vec<PathBuf>,
        remote_dir: Option<PathBuf>,
        /**
         * The directory where the files came from (could be local cwd or the watch_dir, for example)
         * Used to calculate the relative path for the remote upload,
         * see SftpClient::compute_local_relative_filepath
         */
        local_base_dir: Option<PathBuf>,
    },
}

#[derive(Clone)]
pub enum AuthMethod {
    Password(String),
    /**
     * pubkey: PathBuf,
     * privatekey: PathBuf,
     * passphrase: Option<String>,
     */
    Pubkey(PathBuf, PathBuf, Option<String>),
}

impl UploadActor {
    pub fn new(
        rx: StdReceiver<UploadActorMessage>,
        count: u8,
        host: String,
        port: u16,
        username: String,
        auth_method: AuthMethod,
    ) -> Self {
        // Step 1: Validate count
        if count == 0 {
            panic!("Connection count must be 1 at minimum!");
        }

        // Step 2: Create client vars and multiprogress controller
        let mut client_names = vec![];
        for i in 0..count {
            let client_name = format!("sftp_{}", i + 1);
            client_names.push(client_name.clone());
        }

        // Step 3: init progress actor and internally start it's actor thread
        let progress_handler = ProgressActorHandle::new();

        // loop through count, spawn a thread
        // and create the necessary instances of SftpClient and ProgressBar
        let mut tasks = vec![];
        let connections = Arc::new(Mutex::new(Vec::new()));
        for i in 0..count {
            let client_name = client_names.get(i as usize).unwrap().clone();
            let thread = std::thread::Builder::new().name(client_name.clone());

            // thread_* vars will be moved into the thread by compiler
            let mut thread_progress_handler = progress_handler.clone();
            let thread_connections = connections.clone();
            // vars for uploader instance
            let thread_host = host.clone();
            let thread_port = port.clone();
            let thread_username = username.clone();
            let thread_auth_method = auth_method.clone();

            let task = thread.spawn(move || {
                // Create progressbar with ProgressActor
                // TODO: show spinner for connection progress to the sftp server + show errors if connection fails as message for progressbar!
                thread_progress_handler
                    .add_bar(client_name.clone(), 100)
                    .expect("Error adding progressbar to progress actor!");

                match thread_auth_method {
                    AuthMethod::Password(password) => {
                        // Create SftpClient instance with password auth
                        let mut client = SftpClient::with_password(
                            client_name.as_str(),
                            thread_host.as_str(),
                            thread_port,
                            thread_username.as_str(),
                            password.as_str(),
                        );
                        client.connect();
                        thread_connections.lock().unwrap().push(client);
                    }
                    AuthMethod::Pubkey(pubkey, privatekey, passphrase) => {
                        // Create SftpClient instance with pubkey auth
                        let mut client = SftpClient::new(
                            client_name.as_str(),
                            thread_host.as_str(),
                            thread_port,
                            thread_username.as_str(),
                            pubkey,
                            privatekey,
                            passphrase,
                        );
                        client.connect();
                        thread_connections.lock().unwrap().push(client);
                    }
                }

                // thread_connections lock will be released due to scope end dropping the lock
            });

            tasks.push(task.expect("Error spawning thread!"));
        }

        for task in tasks {
            task.join().expect("Error joining a thread!");
        }

        let connections_unwrapped = match Arc::try_unwrap(connections)
            .ok()
            .map(|mutex| mutex.into_inner().unwrap())
        {
            Some(connections) => connections
                .into_iter()
                .map(|client| Arc::new(Mutex::new(client)))
                .collect(),
            None => panic!("Error unwrapping connections mutex!"),
        };

        Self {
            msg_rx: rx,
            connection_count: count,
            client_names,
            connections: connections_unwrapped,
            progress_handler,
        }
    }

    /**
     *  This function is the main loop of the actor, it will run inside a thread until the actor is dropped!
     *  Important: This function must own the actor (no `&mut self``, but only `mut self``)
     */
    pub fn run_self(mut self) {
        // While loop ends when the sender part (aka msg_tx) is dropped
        while let Ok(msg) = self.msg_rx.recv() {
            match msg {
                // Case 1: Receive files to upload
                // TODO: transform SftpClient into an actor as well, but with real threads!
                UploadActorMessage::UploadFiles {
                    files,
                    remote_dir,
                    local_base_dir,
                } => self.actor_upload_files(files, remote_dir, local_base_dir),
            }
        }
    }

    fn actor_upload_files(
        &mut self,
        files_to_upload: Vec<PathBuf>,
        target_dir: Option<PathBuf>,
        local_base_dir: Option<PathBuf>,
    ) {
        // Step 1: Ignore empty upload events
        if files_to_upload.len() == 0 {
            return;
        }

        // FOR DEBUGGING
        // let message = format!(
        //     "Debug: Uploading {:?} files to {:?}",
        //     files_to_upload.len(),
        //     target_dir
        // );
        // self.actor_print_ln(message);

        // Step 2: Log the upload event
        let upload_event_ts = Local::now();
        let msg = format!(
            "Detected Files to upload: {} - {} ",
            files_to_upload.len(),
            upload_event_ts.format("%H:%M:%S (%Y-%m-%d)").to_string(),
        );
        self.actor_print_ln(msg);

        // Step 3: prepare remote path tree
        // - Base Problem: If each file creates it's own parent dir path tree,
        //   two or more paths might attempt to create the same dir at the same time.
        //   To solve this I would need to add retry logic to the SftpClient::upload_file_to_dir_special function,
        //   which would make this function more complex and harder to maintain.
        // - Instead: I calculate all remote paths upfront and create the necessary directories before uploading the files.
        // TODO: Add progressbar for path creation
        let mut remote_paths = HashSet::new();
        // get a sftp connection (any one)
        let mut path_tree_client = self.connections.first().unwrap().lock().unwrap();

        // Step 3.1: calculate all remote dirs
        for file in files_to_upload.iter() {
            // Note: all paths in files_to_upload are absolute file paths and do not contain dirs
            let mut remote_path = path_tree_client.local_to_remote_path(
                file.as_path(),
                local_base_dir.as_ref().map(|p| p.as_path()),
                target_dir.as_ref().map(|p| p.as_path()),
            );
            remote_path = match remote_path {
                Ok(p) => Ok(p),
                Err(e) => {
                    self.actor_print_ln(format!("Error converting local to remote path: {:?}", e));
                    continue;
                }
            };

            // bjesuiter: .unwrap is safe here, because the path is defined at that point
            let remote_dir = match remote_path.unwrap().parent() {
                Some(p) => p.to_path_buf(),
                None => PathBuf::from("."),
            };
            remote_paths.insert(remote_dir);
        }

        // Step 3.2: create the remote path tree
        // Note: this loop may be slow, in case many dirs need to be created and when many paths have the same path components,
        // since the SftpClient::ensure_dir_remote function checks the existence of each path component.
        // If this is a real speed issue, deduplicate paths based on their components.
        for path in remote_paths.iter() {
            // create the remote path tree
            // TODO: add proper progressbar for path creation
            self.actor_print_ln(format!("Ensure remote path: {:?}", path));

            match path_tree_client.ensure_dir_remote_cached(path) {
                Ok(_) => {}
                Err(e) => {
                    self.actor_print_ln(format!("Error creating remote path: {:?}", e));
                }
            }
        }

        // Step 3.3: free reference to my sftp client for preparing the remote path tree
        drop(path_tree_client);

        // Step 4: split to n chunks
        let chunks = split_to_n_chunks(
            files_to_upload.into_iter().collect(),
            self.connection_count as usize,
        );

        // Step 4.2: Reset progressbars elapsed time - BROKEN: Resets all elapsed times AFTER uploading and not before
        // for i in 0..self.connection_count {
        //     self.progress_handler
        //         .reset_bar_elapsed(i as usize)
        //         .expect("Error resetting progressbar elapsed time!");
        // }

        // Step 5: Go through each chunk and upload them
        // create space for the thread handles BEFORE looping over the chunks
        let mut tasks = vec![];
        for (i, chunk) in chunks.iter().enumerate() {
            // Step 1 per Chunk - Set progressbar length to the number of files in the chunk
            self.progress_handler
                .set_bar_length(i, chunk.len() as u64)
                .expect("Error setting progressbar length!");

            // Step 2 per Chunk - Prepare vars for thread
            // all thread-* vars will be moved into the thread by compiler
            let thread_client_arc = self.connections[i].clone();
            let thread_chunk = chunk.clone();
            let thread_target_dir = target_dir.clone();
            let thread_name = self.client_names[i].clone();
            let thread_local_base_dir = local_base_dir.clone();
            let mut thread_progress_handler = self.progress_handler.clone();

            // Step 3 per Chunk - Spawn the thread
            let thread = std::thread::Builder::new().name(thread_name.to_string());
            let task = thread.spawn(move || {
                let mut thread_client = thread_client_arc.lock().unwrap();
                for file in thread_chunk {
                    // pre upload - prepare progressbar
                    let msg = format!("Uploading: {:?}", file);
                    thread_progress_handler
                        .set_bar_msg(i, msg)
                        .expect("Error setting progressbar msg!");

                    // while upload
                    let sync_result = match &thread_target_dir {
                        None => thread_client.sync_file_to_cwd(
                            file.as_path(),
                            thread_local_base_dir.as_ref().map(|p| p.as_path()),
                            true,
                        ),
                        Some(target_dir) => thread_client.sync_file_to_dir(
                            file.as_path(),
                            target_dir.as_path(),
                            thread_local_base_dir.as_ref().map(|p| p.as_path()),
                            true,
                        ),
                    };

                    match sync_result {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error uploading file: {:?}, {:?}", file.display(), e);
                        }
                    };

                    // after upload - inc progressbar
                    thread_progress_handler
                        .inc_bar_pos(i, 1)
                        .expect("Error incrementing progressbar!");
                }
                // thread_client will be dropped here, releasing the lock for this specific SftpClient
                // => does not block other threads from accessing their SftpClient
            });

            tasks.push(task);
        }

        // Step 6: Wait for all threads to finish
        for (i, task) in tasks.into_iter().enumerate() {
            match task {
                Ok(task) => {
                    if let Err(error) = task.join() {
                        self.progress_handler
                            .print_ln("Error joining a file upload thread!".to_string());
                    };

                    // finish the progressbar after upload threads are done
                    self.progress_handler
                        .finish_bar(i, "Finished uploading files!".to_string())
                        .expect("Error finishing progressbar!");
                }
                Err(e) => {
                    println!("Error spawning thread: {:?}", e);
                }
            }
        }
    }

    fn actor_print_ln(&self, message: String) {
        let send_result = self.progress_handler.print_ln(message);

        if let Err(e) = send_result {
            // bjesuiter: panicing here, because using println macro fails here
            // due to complete control over stdout from progress_actor
            panic!("Error sending message to progress actor: {:?}", e);
        }
    }
}
