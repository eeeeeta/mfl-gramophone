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
    Ping,
    /// /file/NAME/play LEVEL
    ///
    /// Starts playing a file.
    ///
    /// - LEVEL: the volume level, in decibels, to begin playback at
    PlayFile(String, f64),
    /// /file/NAME/fade LEVEL DURATION
    ///
    /// Fades the volume of a file.
    ///
    /// - LEVEL: the volume level, in decibels, to fade to
    /// - DURATION: the duration, in milliseconds, for the fade
    FadeFile(String, f64, u64),
    /// /file/NAME/stop
    /// 
    /// Stops playback.
    StopFile(String),
    /// /file/NAME/debug
    ///
    /// Prints debug information to the logs.
    DebugFile(String),
    Engine(AudioThreadMessage),
    BufferComplete(String, u32)
}
/// Converts a linear amplitude to decibels.
pub fn lin_db(lin: f64) -> f64 {
    lin.log10() * 20.0
}
/// Converts a decibel value to a linear amplitude.
pub fn db_lin(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}
pub enum BufferingMessage {
    Continue,
    Die
}

pub struct ActiveFile {
    senders: Vec<PlainSender>,
    buffered: bool,
    epoch: u32,
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
    pub epoch: u32
}
impl Context {
    pub fn process_message(&mut self, msg: Message) -> Result<(), Error> {
        use self::Message::*;

        match msg {
            Shutdown => self.shutdown(),
            Ping => info!("Ping received"),
            PlayFile(st, level) => {
                self.prepare_file(&st, level)?;
                self.start_stop_file(&st, true)?;
            },
            StopFile(st) => {
                self.start_stop_file(&st, false)?;
            },
            DebugFile(st) => {
                self.debug_file(&st)?;
            },
            FadeFile(st, target, dur_ms) => {
                self.configure_file_fade(&st, target, dur_ms)?;
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
            BufferComplete(st, epo) => {
                if let Some(fi) = self.active_files.get_mut(&st) {
                    if epo == fi.epoch {
                        fi.buffered = true;
                    }
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
    pub fn debug_file(&mut self, file: &str) -> Result<(), Error> {
        info!("Debugging state for file '{}'", file);
        let file = self.active_files.get_mut(file)
            .ok_or(format_err!("No such active file."))?;
        info!("senders: {}", file.senders.len());
        info!("buffered: {}", file.buffered);
        info!("sender 0 alive: {}", file.senders[0].alive());
        info!("sender 0 active: {}", file.senders[0].active());
        info!("sender 0 position_samples: {}", file.senders[0].position_samples());
        info!("volume: {:?}", file.senders[0].volume());
        Ok(())
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
    pub fn configure_file_fade(&mut self, file: &str, target: f64, dur_ms: u64) -> Result<(), Error> {
        info!("Configuring fade (target {:.02}dB, dur {}) for file '{}'", target, dur_ms, file);
        let target = db_lin(target);
        let file = self.active_files.get_mut(file)
            .ok_or(format_err!("No such active file."))?;
        let time = PlainSender::precise_time_ns();
        let cur_vol = file.senders[0].volume().get(time);
        let mut fd = FadeDetails::new(cur_vol, target as _);
        fd.set_start_time(time);
        fd.set_duration(::std::time::Duration::from_millis(dur_ms));
        fd.set_active(true);
        for ch in file.senders.iter_mut() {
            ch.set_volume(Box::new(Parameter::LinearFade(fd.clone())));
        }
        Ok(())
    }
    pub fn prepare_file(&mut self, file: &str, level: f64) -> Result<(), Error> {
        info!("Preparing to play file '{}' at level {:.02}dB", file, level);
        let level = db_lin(level);
        let filename = file.to_string();
        let filename2 = filename.clone();
        let file = self.cfg.files.get(file).ok_or(format_err!("No such file."))?;
        let mut mf = MediaFile::new(&mut self.mctx, &file.uri)?;
        let mut senders = vec![];
        let mut ctls = vec![];
        for i in 0..self.cfg.channels.len() {
            let mut send = self.ec.new_sender(mf.sample_rate() as u64);
            send.set_output_patch(i);
            send.set_volume(Box::new(Parameter::Raw(level as _)));
            ctls.push(send.make_plain());
            senders.push(send);
        }
        let txc = self.tx.clone();
        let (btx, brx) = channel();
        self.epoch += 1;
        let epo = self.epoch;
        ::std::thread::spawn(move || {
            info!("Starting buffering thread for file '{}' epoch {}", filename, epo);
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
            info!("File '{}' epoch {} finished buffering", filename, epo);
            txc.send(Message::BufferComplete(filename.clone(), epo)).unwrap();
            while let Ok(x) = brx.recv() {
                if let BufferingMessage::Die = x {
                    break;
                }
            }
            info!("File '{}' epoch {} buffer thread stopped", filename, epo);
        });
        self.active_files.insert(filename2, ActiveFile {
            senders: ctls,
            buffered: false,
            epoch: self.epoch,
            tx: btx
        });
        Ok(())
    }
    pub fn shutdown(&mut self) -> ! {
        warn!("Shutting down...");
        panic!("Shutdown requested!");
    }
    pub fn run(&mut self) -> ! {
        info!("[+] Up and running!");
        loop {
            let res = self.rx.recv();
            match res {
                Err(_) => {
                    panic!("Channel split; performing shutdown");
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
