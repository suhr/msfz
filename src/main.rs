#![allow(dead_code)]

extern crate hound;
//extern crate num_complex;
extern crate jack;
extern crate miosc;
extern crate rosc;
extern crate resample;
extern crate monochord;

//use jack::prelude::*;
use std::net::UdpSocket;
use std::sync::mpsc;

use std::error;
use monochord::{Cents, Hz};

const MIDI_REF: Hz = Hz(8.1757989156);

mod pvoc;

enum MioscIoError {
    IoError(::std::io::Error),
    OscError(rosc::OscError),
    MioscError(::miosc::MioscError),
}

impl From<::std::io::Error> for MioscIoError {
    fn from(source: std::io::Error) -> Self {
        MioscIoError::IoError(source)
    }
}

impl From<::miosc::MioscError> for MioscIoError {
    fn from(source: miosc::MioscError) -> Self {
        MioscIoError::MioscError(source)
    }
}

impl From<::rosc::OscError> for MioscIoError {
    fn from(source: rosc::OscError) -> Self {
        MioscIoError::OscError(source)
    }
}

struct SampleBank {
    samples: Vec<Sample>,
}

struct Sample {
    path: std::path::PathBuf,
    freq: Hz,
    pitch_range: (Cents, Cents),
}

impl Sample {
}

struct Engine {
    resampler: resample::Resampler,
    sola: pvoc::Sola,
}

impl Engine {
    fn new() -> Self {
        Self {
            resampler: resample::Resampler::new(),
            sola: pvoc::Sola::new(),
        }
    }
    fn get_chunk(&mut self, sample: &Sample, pitch: Cents) -> Chunk {
        let wave = open_wav(&sample.path);

        let new_freq = MIDI_REF + pitch;
        let ratio = new_freq.0 / sample.freq.0;

        let shifted =
            if ratio >= 1.0 {
                let streched = self.sola.process(&wave, ratio as _, sample.freq.0 as _);
                self.resampler.resample(&streched, ratio as _)
            }
            else {
                let resampled = self.resampler.resample(&wave, ratio as _);
                self.sola.process(&resampled, ratio as _, new_freq.0 as _)
            };

        Chunk {
            wave: shifted,
            pos: 0,
        }
    }
}

struct Chunk {
    wave: Vec<f64>,
    pos: usize,
}

impl Chunk {
    fn new(wave: Vec<f64>) -> Self {
        Chunk {
            wave,
            pos: 0,
        }
    }

    fn feed(&mut self, out: &mut [f32]) {
        if self.pos >= self.wave.len() {return }

        let slice = &self.wave[self.pos..];
        for (v, w) in out.iter_mut().zip(slice) {
            *v = *w as f32;
        }

        self.pos += out.len()
    }
}

enum Msg {
    Play(Chunk),
    Stop,
}

fn open_wav(path: &std::path::Path) -> Vec<f64> {
    let mut reader = hound::WavReader::open(path).unwrap(); //clarinet-d4.wav
    let wave: Vec<_> = reader.samples::<i16>().map(|s| s.unwrap() as f64 / ::std::i16::MAX as f64).collect();
    let wave: Vec<_> = wave.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect();

    wave
}

fn read_miosc(socket: &UdpSocket) -> Result<miosc::MioscMessage, MioscIoError> {
    let mut buf = [0u8; 1024];

    let (n, _) = socket.recv_from(&mut buf)?;
    let pkg = rosc::decoder::decode(&buf[..n])?;

    match pkg {
        rosc::OscPacket::Message(msg) =>
            Ok(miosc::into_miosc(msg)?),
        _ => unimplemented!()
    }
}

fn main() {
    let (client, _status) = jack::Client::new("msfz", jack::ClientOptions::NO_START_SERVER).unwrap();
    let mut out_port = client.register_port("mono", jack::AudioOut::default()).unwrap();

    let (tx, rx) = mpsc::channel();
    let mut chunk: Option<Chunk> = None;
    let process = jack::ClosureProcessHandler::new(move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let chunk = &mut chunk;
        let out = out_port.as_mut_slice(ps);

        match rx.try_recv() {
            Ok(Msg::Play(s)) =>
                *chunk = Some(s),
            Ok(Msg::Stop) =>{
                *chunk = None;
                for v in out.iter_mut() {
                    *v = 0.0;
                }
            },
            _ => (),
        }

        if let &mut Some(ref mut s) = chunk {
            s.feed(out);
        }

        jack::Control::Continue
    });

    let active_client = client.activate_async((), process).unwrap();

    let socket = UdpSocket::bind("127.0.0.1:3579").unwrap();
    let mut engine = Engine::new();

    loop {
        use miosc::MioscMessage as MM;
        let msg = read_miosc(&socket);
        // let sample = Sample {
        //     path: "marimba-c5.wav".into(),
        //     freq: Hz(523.25),
        //     pitch_range: (Cents(7200.0), Cents(8400.0)),
        // };
        let sample = Sample {
            path: "clarinet-d4.wav".into(),
            freq: Hz(293.66),
            pitch_range: (Cents(6200.0), Cents(7400.0)),
        };

        match msg {
            Ok(MM::NoteOn(_id, pitch, _vel)) => {
                let chunk = engine.get_chunk(&sample, sample.pitch_range.0 + Cents(100.0 * pitch));
                drop(tx.send(Msg::Play(chunk)))
            },
            Ok(MM::NoteOff(_id)) => {
                drop(tx.send(Msg::Stop));
            },
            _ => (),
        }

        let dt = ::std::time::Duration::from_millis(8);
        ::std::thread::sleep(dt);
    }

    active_client.deactivate().unwrap();
}
