#[macro_use] extern crate failure;
extern crate rosc;
extern crate sqa_engine;
extern crate sqa_ffmpeg;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate config as cfg;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate uuid;

pub mod config;
pub mod ctxt;
pub mod osc;

use sqa_engine::EngineContext;
use sqa_ffmpeg::MediaFile;
use std::net::UdpSocket;
use std::sync::mpsc;
use std::collections::HashMap;

fn main() {
    env_logger::init();
    info!("[+] mfl-gramophone starting up");
    info!("[+] Loading configuration");
    let cfg = config::Config::get().unwrap();
    if cfg.channels.len() == 0 {
        panic!("No channels configured.");
    }
    if cfg.files.len() == 0 {
        warn!("No files configured!");
    }
    info!("[+] Initialising SQA Engine");
    let mut ec = EngineContext::new(Some("mfl-gramophone")).unwrap();
    if ec.conn.sample_rate() as u64 != cfg.sample_rate {
        panic!("JACK sample rate ({}) doesn't match configured ({})", ec.conn.sample_rate(), cfg.sample_rate);
    }
    let (tx, rx) = mpsc::channel();
    let mut hdl = ec.get_handle().unwrap();
    let txc = tx.clone();
    ::std::thread::spawn(move || {
        loop {
            let msg = hdl.recv();
            txc.send(ctxt::Message::Engine(msg)).unwrap();
        }
    });
    let mut chans = vec![];
    for (i, ch) in cfg.channels.iter().enumerate() {
        info!("[+] Setting up channel {} (-> {})...", i, ch);
        let st = format!("channel {}", i);
        let p = ec.new_channel(&st).expect("making channel failed");
        let port = ec.conn.get_port_by_name(&ch).expect("getting port failed");
        ec.conn.connect_ports(ec.chans.get(p).unwrap().as_ref().unwrap(), &port)
            .expect("patching failed");
        chans.push(p);
    }
    info!("[+] Initialising FFmpeg");
    let mut mctx = ::sqa_ffmpeg::init().unwrap();
    info!("[+] Checking configured media files");
    for (name, pf) in cfg.files.iter() {
        info!("[+] Checking '{}' ({})...", name, pf.uri);
        let mf = MediaFile::new(&mut mctx, &pf.uri).expect("failed opening file");
        if mf.sample_rate() as u64 != cfg.sample_rate {
            panic!("File '{}' has sample rate {} (needed {})", name, mf.sample_rate(), cfg.sample_rate);
        }
        if mf.channels() == 0 {
            panic!("File '{}' has no channels");
        }
        if mf.channels() > cfg.channels.len() {
            warn!("File '{}' has more channels ({}) than configured ({}); some will not play!", name, mf.channels(), cfg.channels.len());
        }
    }
    info!("[+] Initialising OSC");
    let sock = UdpSocket::bind(&cfg.listen).expect("failed binding socket");
    let txc = tx.clone();
    let mut osc_ctxt = osc::OscContext { sock, tx: txc };
    ::std::thread::spawn(move || {
        osc_ctxt.run();
    });
    let mut ctx = ctxt::Context { 
        rx, ec, mctx, cfg, tx,
        active_files: HashMap::new()
    };
    ctx.run();
}
