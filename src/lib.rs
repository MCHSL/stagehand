#[macro_use]
extern crate lazy_static;

use artnet_protocol::*;
use auxcallback::{callback_sender_by_id, callback_sender_by_id_insert};
use auxtools::*;
use dashmap::DashMap;
use std::convert::TryInto;
use std::net::{ToSocketAddrs, UdpSocket};
use std::thread;

lazy_static! {
    static ref UNIVERSES: DashMap<PortAddress, Universe> = DashMap::new();
}

struct DMXReceiver {
    target: raw_types::values::Value,
    proc: String,
    start_channel: usize,
    end_channel: usize,
}

impl DMXReceiver {
    fn is_affected(&self, channels: &Vec<usize>) -> bool {
        channels
            .iter()
            .any(|c| self.start_channel <= *c && *c <= self.end_channel)
    }
}

impl std::fmt::Debug for DMXReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}-{}",
            self.proc, self.start_channel, self.end_channel
        )
    }
}

impl Clone for DMXReceiver {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            proc: self.proc.clone(),
            start_channel: self.start_channel,
            end_channel: self.end_channel,
        }
    }
}

struct Universe {
    receivers: Vec<DMXReceiver>,
    last_frame: Vec<u8>,
}

impl Universe {
    fn send(&mut self, data: &Vec<u8>) {
        let cb_sender = callback_sender_by_id("stagehand".into()).unwrap();

        let delta = self.get_changed_channels(data);
        let mut receivers = self.receivers.clone();
        receivers.retain(|r| r.is_affected(&delta));

        if !receivers.is_empty() {
            let channels: Vec<f32> = data.iter().map(|u| *u as f32).collect();
            cb_sender
                .send(Box::new(move || {
                    let data: Vec<Value> = channels.iter().map(|x| Value::from(*x)).collect();
                    let bruh: Vec<&Value> = data.iter().map(|v| v).collect();
                    for receiver in receivers.iter() {
                        let target = unsafe { Value::from_raw(receiver.target) };
                        target
                            .call(
                                &receiver.proc,
                                &bruh[receiver.start_channel..=receiver.end_channel],
                            )
                            .unwrap();
                    }
                    Ok(Value::null())
                }))
                .unwrap();
        }

        self.last_frame = data.clone();
    }

    fn add_receiver(&mut self, receiver: DMXReceiver) {
        self.receivers.push(receiver);
    }

    fn get_changed_channels(&self, frame: &Vec<u8>) -> Vec<usize> {
        if self.last_frame.is_empty() {
            return (0..frame.len()).collect(); // If this is the first frame, we can assume all channels have been modified
        }
        let mut delta = Vec::with_capacity(16);
        self.last_frame
            .iter()
            .zip(frame.iter())
            .enumerate()
            .for_each(|(i, (last, cur))| {
                if *last != *cur {
                    delta.push(i)
                }
            });
        delta
    }
}

impl Default for Universe {
    fn default() -> Self {
        Self {
            receivers: vec![],
            last_frame: vec![],
        }
    }
}

fn handle_messages() {
    // Define reciever socket
    let socket = UdpSocket::bind(("0.0.0.0", 6454)).unwrap();

    // Send a broadcast to tell other devices we are an artnet node
    let broadcast_addr = ("255.255.255.255", 6454)
        .to_socket_addrs()
        .unwrap()
        .next()
        .unwrap();
    socket.set_broadcast(true).unwrap();
    let buff = ArtCommand::Poll(Poll::default()).write_to_buffer().unwrap();
    socket.send_to(&buff, &broadcast_addr).unwrap();

    // Cache of stuff
    //let mut dmx_cache = HashMap::new();

    // Do all data pulling in here
    loop {
        let mut buffer = [0u8; 1024];
        let (length, addr) = socket.recv_from(&mut buffer).unwrap();
        let command = ArtCommand::from_buffer(&buffer[..length]).unwrap();

        //println!("Received {:?}", command);
        match command {
            ArtCommand::Poll(_poll) => {
                // This will most likely be our own poll request, as this is broadcast to all devices on the network
            }
            ArtCommand::PollReply(_reply) => {
                // This is an ArtNet node on the network. We can send commands to it like this:
                let command = ArtCommand::Output(Output {
                    data: vec![1, 3, 3, 7].into(), // The data we're sending to the node
                    ..Output::default()
                });
                let bytes = command.write_to_buffer().unwrap();
                socket.send_to(&bytes, &addr).unwrap();
            }
            ArtCommand::Output(out) => {
                let data = &out.data.inner;
                if let Some(mut universe) = UNIVERSES.get_mut(&out.port_address) {
                    universe.send(data);
                }
                // No cache, lets put it in
                /*if !dmx_cache.contains_key(&universe) {
                    dmx_cache.insert(universe, data.clone());
                    println!("Inserting universe {:?} into cache", universe);
                }

                let universe_cache = dmx_cache.get(&universe).unwrap();
                let mut equal = true;
                let mut i : u16 = 0;
                while i < 512 {
                    if universe_cache[i as usize] != data[i as usize] {
                        equal = false;
                        println!("Values changed!");
                        break
                    }
                    i += 1;
                }

                if !equal {
                    dmx_cache. (universe).insert(data.clone());
                    println!("Fixture Data {:?}", &data);
                    println!("Universe {:?}", &universe);
                }*/
            }
            _ => {}
        }
    }
}

#[hook("/proc/enable_stagehand")]
fn enable_stagehand() {
    // Ensure we have the channel ready before starting the other thread
    // Otherwise, instant deadlock from inserting stuff into dashmap while reading it
    callback_sender_by_id_insert("stagehand".to_string());
    thread::spawn(|| handle_messages());
    Ok(Value::from(true))
}

#[hook("/proc/dmx_register")]
fn dmx_register(
    thing: Value,
    procpath: Value,
    universe: Value,
    start_channel: Value,
    footprint: Value,
) {
    // If you pass incorrect arguments I will smite you (and crash)
    let target = thing.raw;
    unsafe {
        raw_types::funcs::inc_ref_count(target);
    } // Please don't murder me willox

    let proc = procpath.to_string()?.split("/").last().unwrap().to_owned();

    let universe = universe.as_number()? as u16;
    let start_channel = start_channel.as_number()? as usize - 1;
    let end_channel = start_channel + footprint.as_number()? as usize - 1;

    UNIVERSES
        .entry(universe.try_into().unwrap())
        .or_default()
        .add_receiver(DMXReceiver {
            target,
            proc,
            start_channel,
            end_channel,
        });
    Ok(Value::from(true))
}
