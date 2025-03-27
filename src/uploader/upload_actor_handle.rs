use super::upload_actor::{AuthMethod, UploadActor, UploadActorMessage};
use std::{
    path::PathBuf,
    sync::mpsc::{channel as std_channel, SendError, Sender as StdSender},
};

#[derive(Clone)]
pub struct UploadActorHandle {
    tx: StdSender<UploadActorMessage>,
}

impl UploadActorHandle {
    pub fn new(
        count: u8,
        host: String,
        port: u16,
        username: String,
        auth_method: AuthMethod,
    ) -> Self {
        let (tx, rx) = std_channel();

        // Create the actor and pass the channel receiver (rx)
        let actor = UploadActor::new(rx, count, host, port, username, auth_method);

        // spawn the actor
        let thread = std::thread::Builder::new().name("upload_actor_main".to_string());
        if let Err(error) = thread.spawn(|| actor.run_self()) {
            eprintln!("Error spawning the upload actor main thread: {:?}", error);
        };

        // Create the UploadActorHandle object and store the sender (tx)
        Self { tx }
    }

    pub fn upload_files(
        &mut self,
        files: Vec<PathBuf>,
        remote_dir: Option<PathBuf>,
        local_base_dir: Option<PathBuf>,
    ) -> Result<(), SendError<UploadActorMessage>> {
        let msg = UploadActorMessage::UploadFiles {
            files,
            remote_dir,
            local_base_dir,
        };
        self.tx.send(msg)?;
        Ok(())
    }
}
