use std::net::{UdpSocket, ToSocketAddrs};
use std::sync::mpsc::Sender;
use rosc::{OscMessage, OscPacket, OscType};
use ctxt::Message;
use failure::Error;

pub struct OscContext {
    pub sock: UdpSocket,
    pub tx: Sender<Message>,
}

impl OscContext {
    fn process_msg<A: ToSocketAddrs>(&mut self, msg: OscMessage, from: A) {
        let addr = msg.addr.trim().split("/").collect::<Vec<_>>();
        if addr.len() == 1 {
            return;
        }
        match addr[1] {
            "ping" => {
                info!("Got a ping");
                self.send(OscMessage {
                    addr: "/pong".into(),
                    args: None
                }, from);
            },
            "file" => {
                if addr.len() > 3 {
                    match addr[3] {
                        "start" => {
                            self.tx.send(Message::PlayFile(addr[2].into()))
                                .unwrap();
                            self.send_ack(from);
                        },
                        "stop" => {
                            self.tx.send(Message::StopFile(addr[2].into()))
                                .unwrap();
                            self.send_ack(from);
                        },
                        x @ "fade_in" | x @ "fade_out" => {
                            if let Some(OscType::Int(dur)) = msg.args.and_then(|x| x.get(0).map(|x| x.clone())) {
                                if x == "fade_in" {
                                    self.tx.send(Message::FadeInFile(addr[2].into(), dur as _))
                                        .unwrap();
                                }
                                else {
                                    self.tx.send(Message::FadeOutFile(addr[2].into(), dur as _))
                                        .unwrap();
                                }
                                self.send_ack(from);
                            }
                            else {
                                warn!("Incorrect arguments provided for fade command");
                                self.send(OscMessage {
                                    addr: "/incorrect_args".into(),
                                    args: Some(vec![OscType::String(addr.join("/"))])
                                }, from);
                            }
                        },
                        _ => {
                            self.send_unknown(addr.join("/"), from);
                        }
                    }
                }
            },
            _ => {
                self.send_unknown(addr.join("/"), from);
            }
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
