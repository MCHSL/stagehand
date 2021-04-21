#[macro_use]
extern crate lazy_static;

use artnet_protocol::*;
use auxcallback::{callback_sender_by_id, callback_sender_by_id_insert};
use auxtools::*;
use dashmap::DashMap;
use std::convert::TryInto;
use std::net::UdpSocket;
use std::thread;

lazy_static! {
    static ref UNIVERSES: DashMap<PortAddress, Universe> = DashMap::new();
}

struct DMXFixture {
    target: raw_types::values::Value,
    proc: String,
    start_channel: usize,
    end_channel: usize,
}

impl DMXFixture {
    fn is_affected(&self, channels: &Vec<usize>) -> bool {
        channels
            .iter()
            .any(|c| self.start_channel <= *c && *c <= self.end_channel)
    }
}

impl std::fmt::Debug for DMXFixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}-{}",
            self.proc, self.start_channel, self.end_channel
        )
    }
}

impl Clone for DMXFixture {
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
    receivers: Vec<DMXFixture>,
    last_frame: Vec<u8>,
}

impl Universe {
    fn send(&mut self, data: &Vec<u8>) {
        let cb_sender = callback_sender_by_id("stagehand".into()).unwrap();

        let delta = self.get_changed_channels(data);
        let receivers: Vec<DMXFixture> = self
            .receivers
            .clone()
            .into_iter()
            .filter(|r| r.is_affected(&delta))
            .collect();

        if !receivers.is_empty() {
            let channels: Vec<f32> = data.iter().map(|u| *u as f32).collect();
            let _ = cb_sender.send(Box::new(move || {
                let data: Vec<Value> = channels.iter().map(|x| Value::from(*x)).collect();
                let bruh: Vec<&Value> = data.iter().map(|v| v).collect();
                for receiver in receivers.iter() {
                    let target = unsafe { Value::from_raw(receiver.target) };
                    target.call(
                        &receiver.proc,
                        &bruh[receiver.start_channel..=receiver.end_channel],
                    )?;
                }
                Ok(Value::null())
            }));
        }

        self.last_frame = data.clone();
    }

    fn add_receiver(&mut self, receiver: DMXFixture) {
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

fn send_error(err: String) {
    let _ = callback_sender_by_id("stagehand".into())
        .unwrap()
        .send(Box::new(move || {
            // If you don't have this proc, set up your codebase for
            // auxtools before trying to use libraries based on it
            let _ = Proc::find("/proc/auxtools_stack_trace")
                .unwrap()
                .call(&[&Value::from_string(err.clone()).unwrap()]);
            Ok(Value::null())
        }));
}

fn handle_messages() {
    let socket = match UdpSocket::bind(("0.0.0.0", 6454)) {
        Ok(s) => s,
        Err(e) => {
            send_error(e.to_string());
            return;
        }
    };

    loop {
        let mut buffer = [0u8; 1024];
        let length = match socket.recv(&mut buffer) {
            Ok(length) => length,
            Err(e) => {
                send_error(e.to_string());
                return;
            }
        };
        let command = match ArtCommand::from_buffer(&buffer[..length]) {
            Ok(c) => c,
            Err(e) => {
                send_error(e.to_string());
                return;
            }
        };

        match command {
            ArtCommand::Output(out) => {
                if let Some(mut universe) = UNIVERSES.get_mut(&out.port_address) {
                    universe.send(&out.data.inner);
                }
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

    let proc = procpath
        .to_string()?
        .split("/")
        .last()
        .ok_or_else(|| runtime!("Invalid proc path passed to dmx_register"))?
        .to_owned();

    let universe = universe.as_number()? as u16;
    let start_channel = start_channel.as_number()? as usize;
    if start_channel == 0 {
        return Err(runtime!("Start channel must be greater than 0"));
    }
    let start_channel = start_channel - 1;

    let footprint = footprint.as_number()? as usize;
    if footprint == 0 {
        return Err(runtime!("Footprint must be greater than 0"));
    }

    let end_channel = start_channel + footprint - 1;

    UNIVERSES
        .entry(
            universe
                .try_into()
                .map_err(|_e| runtime!("Invalid universe ID passed to dmx_register"))?,
        )
        .or_default()
        .add_receiver(DMXFixture {
            target,
            proc,
            start_channel,
            end_channel,
        });
    Ok(Value::from(true))
}
