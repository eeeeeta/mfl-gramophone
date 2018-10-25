use std::sync::mpsc::{Sender, Receiver, channel};
use sqa_engine::{EngineContext, PlainSender};
use sqa_ffmpeg::{MediaContext, MediaFile};
use sqa_engine::param::{Parameter, FadeDetails};
use config::Config;
use std::collections::HashMap;
use sqa_engine::sync::AudioThreadMessage;
use failure::Error;

pub enum Message {
    Shutdown,
    FastShutdown,
    PlayFile(String),
    FadeInFile(String, u64),
    StopFile(String),
    FadeOutFile(String, u64),
    Engine(AudioThreadMessage),
    BufferComplete(String)
}
pub enum BufferingMessage {
    Continue,
    Die
}

pub struct ActiveFile {
    senders: Vec<PlainSender>,
    faded_out: bool,
    buffered: bool,
    tx: Sender<BufferingMessage>
}
impl Drop for ActiveFile {
    fn drop(&mut self) {
        let _ = self.tx.send(BufferingMessage::Die);
    }
}

pub struct Context {
    pub tx: Sender<Message>,
    pub rx: Receiver<Message>,
    pub ec: EngineContext,
    pub mctx: MediaContext,
    pub active_files: HashMap<String, ActiveFile>,
    pub cfg: Config,
}
impl Context {
    pub fn process_message(&mut self, msg: Message) -> Result<(), Error> {
        use self::Message::*;

        match msg {
            Shutdown => self.shutdown(),
            FastShutdown => self.fast_shutdown(),
            PlayFile(st) => {
                self.prepare_file(&st)?;
                self.start_stop_file(&st, true)?;
            },
            StopFile(st) => {
                self.start_stop_file(&st, false)?;
            },
            FadeInFile(st, dur_ms) => {
                self.prepare_file(&st)?;
                self.configure_file_fade(&st, true, dur_ms)?;
                self.start_stop_file(&st, true)?;
            },
            FadeOutFile(st, dur_ms) => {
                self.configure_file_fade(&st, false, dur_ms)?;
            },
            Engine(msg) => {
                use self::AudioThreadMessage::*;

                match msg {
                    Xrun => {
                        warn!("Audio thread xrun!");
                    },
                    PlayerInvalidOutpatch(uu) => {
                        warn!("Player {} has invalid outpatch", uu);
                    },
                    PlayerRejected(_) => {
                        error!("Player limit exceeded!");
                    },
                    PlayerBufHalf(uu) => {
                        let name = self.lookup_uu(uu);
                        if let Some(n) = name {
                            if let Err(e) = self.active_files[&n].tx.send(BufferingMessage::Continue) {
                                warn!("Failed sending wakeup for file '{}': {}", n, e);
                            }
                        }
                    }
                    PlayerBufEmpty(uu) => {
                        let name = self.lookup_uu(uu);
                        if let Some(n) = name {
                            if self.active_files[&n].buffered {
                                info!("File '{}' finished playback", n);
                                self.active_files.remove(&n);
                            }
                            else {
                                warn!("File '{}' ran out of samples!", n);
                            }
                        }
                        else {
                            warn!("Straggling player {} ran out of samples", uu);
                        }
                    },
                    _ => {}
                }
            },
            BufferComplete(st) => {
                if let Some(fi) = self.active_files.get_mut(&st) {
                    fi.buffered = true;
                }
            }
        }
        Ok(())
    }
    pub fn lookup_uu(&mut self, uu: ::uuid::Uuid) -> Option<String> {
        let mut name = None;
        for (st, fi) in self.active_files.iter_mut() {
            for ch in fi.senders.iter() {
                if ch.uuid() == uu {
                    name = Some(st.to_owned());
                    break;
                }
            }
        }
        name
    }
    pub fn start_stop_file(&mut self, file: &str, start: bool) -> Result<(), Error> {
        info!("Setting active state to {} for file '{}'", start, file);
        {
            let file = self.active_files.get_mut(file)
                .ok_or(format_err!("No such active file."))?;
            let time = PlainSender::precise_time_ns();
            for ch in file.senders.iter_mut() {
                if start {
                    ch.set_start_time(time);
                    ch.set_active(true);
                }
                else {
                    // kinda redundant but ¯\_(ツ)_/¯
                    ch.set_active(false);
                }
            }
        }
        if !start {
            self.active_files.remove(file);
        }
        Ok(())
    }
    pub fn configure_file_fade(&mut self, file: &str, in_out: bool, dur_ms: u64) -> Result<(), Error> {
        info!("Configuring fade (direction {}, dur {}) for file '{}'", in_out, dur_ms, file);
        let (from, target) = if in_out {
            (0.0, 1.0)
        }
        else {
            (1.0, 0.0)
        };
        let file = self.active_files.get_mut(file)
            .ok_or(format_err!("No such active file."))?;
        let time = PlainSender::precise_time_ns();
        let mut fd = FadeDetails::new(from, target);
        fd.set_start_time(time);
        fd.set_duration(::std::time::Duration::from_millis(dur_ms));
        fd.set_active(true);
        for ch in file.senders.iter_mut() {
            ch.set_volume(Box::new(Parameter::LinearFade(fd.clone())));
        }
        if !in_out {
            file.faded_out = true;
        }
        Ok(())
    }
    pub fn prepare_file(&mut self, file: &str) -> Result<(), Error> {
        info!("Preparing to play file '{}'", file);
        let filename = file.to_string();
        let filename2 = filename.clone();
        let file = self.cfg.files.get(file).ok_or(format_err!("No such file."))?;
        let mut mf = MediaFile::new(&mut self.mctx, &file.uri)?;
        let mut senders = vec![];
        let mut ctls = vec![];
        for i in 0..self.cfg.channels.len() {
            let mut send = self.ec.new_sender(mf.sample_rate() as u64);
            send.set_output_patch(i);
            ctls.push(send.make_plain());
            senders.push(send);
        }
        let txc = self.tx.clone();
        let (btx, brx) = channel();
        ::std::thread::spawn(move || {
            info!("Starting buffering thread for file '{}'", filename);
            'outer: for frame in &mut mf {
                match frame {
                    Ok(mut frame) => {
                        for (ch, smpl) in &mut frame {
                            if let Some(s) = senders.get_mut(ch) {
                                while let Some(_) = s.buf.try_push(smpl.f32()) {
                                    let msg = brx.recv();
                                    match msg {
                                        Ok(BufferingMessage::Continue) => {},
                                        Ok(BufferingMessage::Die) | Err(_) => {
                                            info!("File '{}' buffering ended prematurely", filename);
                                            break 'outer;
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Buffer error for file '{}': {}", filename, e);
                    }
                }
            }
            info!("File '{}' finished buffering", filename);
            txc.send(Message::BufferComplete(filename)).unwrap();
        });
        self.active_files.insert(filename2, ActiveFile {
            senders: ctls,
            faded_out: false,
            buffered: false,
            tx: btx
        });
        Ok(())
    }
    pub fn fast_shutdown(&mut self) -> ! {
        panic!("Fast shutdown requested!")
    }
    pub fn shutdown(&mut self) -> ! {
        warn!("Shutting down...");
        let mut elapsed = ::std::time::Duration::new(0, 0);
        let thresh = ::std::time::Duration::new(self.cfg.shutdown_secs, 0);

        loop {
            let mut cont = false;
            if elapsed >= thresh {
                warn!("Shutting down forcefully due to timeout.");
                break;
            }
            elapsed += ::std::time::Duration::new(1, 0);
            let mut recordings = vec![];
            for (_, file) in self.active_files.iter() {
                if !file.faded_out {
                    for sender in file.senders.iter() {
                        recordings.push(sender.position_samples());
                    }
                }
            }
            ::std::thread::sleep(::std::time::Duration::new(1, 0));
            // Check whether any senders made any progress
            // (i.e. audio is still playing).
            // If yes, don't die just yet (we may interrupt audio)
            let mut i = 0;
            for (_, file) in self.active_files.iter() {
                if !file.faded_out {
                    for sender in file.senders.iter() {
                        if sender.position_samples() != recordings[i] {
                            cont = true;
                        }
                        i += 1;
                    }
                }
            }
            if !cont {
                break;
            }
        }
        panic!("Shutdown requested!");
    }
    pub fn run(&mut self) -> ! {
        info!("[+] Up and running!");
        loop {
            let res = self.rx.recv();
            match res {
                Err(_) => {
                    error!("Channel split; performing shutdown");
                    self.shutdown();
                },
                Ok(m) => {
                    if let Err(e) = self.process_message(m) {
                        warn!("Error handling message: {}", e);
                    }
                }
            }
        }
    }
}
