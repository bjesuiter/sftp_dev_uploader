use std::sync::mpsc::{channel as std_channel, SendError as StdSendError, Sender as StdSender};

use super::progress_actor::{ProgressActor, ProgressActorMessage};

#[derive(Clone)]
pub struct ProgressActorHandle {
    msg_tx: StdSender<ProgressActorMessage>,
}

impl ProgressActorHandle {
    pub fn new() -> Self {
        let (msg_tx, msg_rx) = std_channel();

        // Create the actor and pass the channel receiver (rx)
        let actor = ProgressActor::new(msg_rx);

        // spawn the actor
        let thread = std::thread::Builder::new().name("progress_actor_main".to_string());
        if let Err(error) = thread.spawn(|| actor.run_self()) {
            eprintln!("Error spawning the progress actor main thread: {:?}", error);
        };

        Self { msg_tx }
    }

    pub fn add_bar(&mut self, name: String, length: u64) -> Result<usize, oneshot::RecvError> {
        let (response_tx, response_rx) = oneshot::channel();
        let msg = ProgressActorMessage::AddBar {
            name,
            length,
            response_tx,
        };

        // No need for error checking here.
        // If the actor is dead, the next recv() will also fail.
        let _ = self.msg_tx.send(msg);

        // returns the internal index of the added progress bar for future reference
        response_rx.recv()
    }

    pub fn set_bar_pos(
        &mut self,
        index: usize,
        pos: u64,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::SetBarPos { index, pos };
        self.msg_tx.send(msg)
    }

    pub fn set_bar_length(
        &mut self,
        index: usize,
        length: u64,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::SetBarLength { index, length };
        self.msg_tx.send(msg)
    }

    pub fn inc_bar_pos(
        &mut self,
        index: usize,
        inc: u64,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::IncBarPos { index, inc };
        self.msg_tx.send(msg)
    }

    pub fn set_bar_msg(
        &mut self,
        index: usize,
        msg: String,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::SetBarMsg { index, msg };
        self.msg_tx.send(msg)
    }

    pub fn finish_bar(
        &mut self,
        index: usize,
        text: String,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::FinishBar { index, msg: text };
        self.msg_tx.send(msg)
    }

    pub fn reset_bar_elapsed(
        &mut self,
        index: usize,
    ) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::ResetBarElapsed { index };
        self.msg_tx.send(msg)
    }

    pub fn print_ln(&self, msg: String) -> Result<(), StdSendError<ProgressActorMessage>> {
        let msg = ProgressActorMessage::PrintLn(msg);
        self.msg_tx.send(msg)
    }
}
