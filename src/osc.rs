use std::net::{UdpSocket, ToSocketAddrs};
use std::sync::mpsc::Sender;
use rosc::{OscMessage, OscPacket, OscType, self};
use ctxt::Message;
use failure::Error;

pub struct OscContext {
    pub sock: UdpSocket,
    pub tx: Sender<Message>,
}

impl OscContext {
    fn parse_message(addr: &[&str], args: Option<Vec<OscType>>) -> Option<Message> {
        if addr.len() == 1 {
            return None;
        }
        match addr[1] {
            "ping" => {
                Some(Message::Ping)
            },
            "shutdown" => {
                Some(Message::Shutdown)
            },
            "file" => {
                if addr.len() <= 3 {
                    return None;
                }
                match addr[3] {
                    "start" => {
                        if let Some(args) = args {
                            if args.len() != 1 {
                                return None;
                            }
                            let level: f64;
                            match args[0] {
                                OscType::Float(f) => level = f as _,
                                OscType::Double(f) => level = f as _,
                                _ => return None
                            }
                            Some(Message::PlayFile(addr[2].into(), level))
                        }
                        else {
                            None
                        }
                    },
                    "debug" => {
                        Some(Message::DebugFile(addr[2].into()))
                    },
                    "stop" => {
                        Some(Message::StopFile(addr[2].into()))
                    },
                    "fade" => {
                        if let Some(args) = args {
                            if args.len() != 2 {
                                return None;
                            }
                            let dur_ms: u64;
                            let target: f64;
                            match args[1] {
                                OscType::Int(dur) => dur_ms = dur as _,
                                _ => return None
                            }
                            match args[0] {
                                OscType::Float(f) => target = f as _,
                                OscType::Double(f) => target = f as _,
                                _ => return None
                            }
                            Some(Message::FadeFile(addr[2].into(), target, dur_ms))
                        }
                        else {
                            None
                        }
                    },
                    _ => {
                        None
                    }
                }
            },
            _ => {
                None
            }
        }
    }
    fn process_msg<A: ToSocketAddrs>(&mut self, msg: OscMessage, from: A) {
        info!("Received message: {} ({} args)", msg.addr, msg.args.as_ref().map(|x| x.len()).unwrap_or(0));
        let addr = msg.addr.trim().split("/").collect::<Vec<_>>();
        if let Some(m) = Self::parse_message(&addr, msg.args) {
            self.tx.send(m).unwrap();
            self.send_ack(from);
            info!("ACK sent");
        }
        else {
            self.send_unknown(addr.join("/"), from);
        }
    }
    fn send_ack<A: ToSocketAddrs>(&mut self, a: A) {
        self.send(OscMessage {
            addr: "/ack".into(),
            args: None
        }, a);
    }
    fn send_unknown<A: ToSocketAddrs>(&mut self, addr: String, a: A) {
        warn!("Unknown OSC address: {}", addr);
        self.send(OscMessage {
            addr: "/unknown_address".into(),
            args: Some(vec![OscType::String(addr)])
        }, a);
    }
    fn _send<A: ToSocketAddrs>(&mut self, msg: OscMessage, a: A) -> Result<(), Error> {
        let msg_buf = rosc::encoder::encode(&OscPacket::Message(msg))
            .map_err(|e| format_err!("{:?}", e))?;
        self.sock.send_to(&msg_buf, a)?;
        Ok(())
    }
    fn send<A: ToSocketAddrs>(&mut self, msg: OscMessage, a: A) {
        if let Err(e) = self._send(msg, a) {
            warn!("Failed to send OSC message: {}", e);
        }
    }
    pub fn run(&mut self) -> ! {
        let mut buf = [0u8; rosc::decoder::MTU];

        loop {
            match self.sock.recv_from(&mut buf) {
                Ok((size, addr)) => {
                    info!("Received packet from {} (size: {})", addr, size);
                    match rosc::decoder::decode(&buf[..size]) {
                        Ok(pkt) => {
                            if let OscPacket::Message(m) = pkt {
                                self.process_msg(m, addr);
                            }
                            else {
                                warn!("Received a bundle! (unimplemented)");
                                self.send(OscMessage {
                                    addr: "/no_bundles_please".into(),
                                    args: None
                                }, addr);
                            }
                        },
                        Err(e) => {
                            warn!("Failed to decode: {:?}", e);
                        }
                    }
                },
                Err(e) => {
                    error!("Error receiving from socket: {}", e);
                    self.tx.send(Message::Shutdown).unwrap();
                    ::std::thread::sleep(::std::time::Duration::from_millis(1000));
                },
            }
        }
    }
}
