use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::mpsc::Receiver as StdReceiver;

pub enum ProgressActorMessage {
    AddBar {
        name: String,
        length: u64,
        // Sends back the internal index of the added progress bar
        response_tx: oneshot::Sender<usize>,
    },
    SetBarLength {
        index: usize,
        length: u64,
    },
    SetBarPos {
        index: usize,
        pos: u64,
    },
    IncBarPos {
        index: usize,
        inc: u64,
    },
    SetBarMsg {
        index: usize,
        msg: String,
    },
    FinishBar {
        index: usize,
        msg: String,
    },
    ResetBarElapsed {
        index: usize,
    },
    PrintLn(String),
}

pub struct ProgressActor {
    // Meta for actor
    msg_rx: StdReceiver<ProgressActorMessage>,

    // Actor internal state
    mulitprogress_controller: MultiProgress,
    default_style: ProgressStyle,
    bars: Vec<ProgressBar>,
}

impl ProgressActor {
    pub fn new(msg_rx: StdReceiver<ProgressActorMessage>) -> Self {
        let mulitprogress_controller = MultiProgress::new();
        let bars = vec![];

        let style = ProgressStyle::default_bar()
            .template("{prefix} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {wide_msg} ")
            .expect("Error creating progress bar style!");

        Self {
            msg_rx,
            mulitprogress_controller,
            default_style: style,
            bars,
        }
    }

    /**
     * This function is the main loop of the actor,
     * it will run inside a thread until the actor is dropped!
     * Important: This function must own the actor (no `&mut self``, but only `mut self``)
     */
    pub fn run_self(mut self) {
        // While loop ends when the sender part (aka msg_tx) is dropped
        while let Ok(msg) = self.msg_rx.recv() {
            match msg {
                ProgressActorMessage::AddBar {
                    name,
                    length,
                    response_tx,
                } => {
                    let new_index = self.actor_add_bar(name, length);
                    response_tx.send(new_index).unwrap();
                }
                ProgressActorMessage::SetBarPos { index, pos } => {
                    self.bars[index].set_position(pos)
                }
                ProgressActorMessage::SetBarLength { index, length } => {
                    self.bars[index].set_length(length);
                }
                ProgressActorMessage::IncBarPos { index, inc } => {
                    self.bars[index].inc(inc);
                }
                ProgressActorMessage::SetBarMsg { index, msg } => {
                    self.bars[index].set_message(msg);
                }
                ProgressActorMessage::FinishBar { index, msg } => {
                    self.bars[index].finish_with_message(msg);
                }
                ProgressActorMessage::ResetBarElapsed { index } => {
                    self.bars[index].reset_elapsed();
                }
                ProgressActorMessage::PrintLn(msg) => {
                    if let Err(e) = self.mulitprogress_controller.println(msg) {
                        eprintln!("Error printing message via MultiProgress class: {:?}", e);
                    }
                }
            }
        }
    }

    fn actor_add_bar(&mut self, name: String, length: u64) -> usize {
        let pb = self
            .mulitprogress_controller
            .add(ProgressBar::new(length as u64));
        pb.set_style(self.default_style.clone());
        pb.set_prefix(name);
        // make sure the bar is drawn
        pb.tick();
        self.bars.push(pb);

        // get the index of the new bar
        self.bars.len() - 1
    }
}
