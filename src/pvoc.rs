use std::cmp;

struct Windows<'s> {
    source: &'s [f64],
    size: usize,
    hop: usize,
    pos: usize,
}

impl<'s> ::std::iter::Iterator for Windows<'s> {
    type Item = &'s [f64];

    fn next(&mut self) -> Option<&'s [f64]> {
        if self.pos + self.hop < self.source.len() {
            let top = cmp::min(self.pos + self.size, self.source.len());
            let win = &self.source[self.pos..top];

            self.pos += self.hop;

            Some(win)
        } else {
            None
        }
    }
}

pub fn pad_vec<T>(vec: &mut Vec<T>, block_size: usize)
where T: Clone + Default
{
    let len = vec.len();
    let pad = len % block_size;

    vec.resize(len + pad, Default::default());
}

fn hann(n: usize, len: usize) -> f64 {
    let tau = n as f64 / (len - 1) as f64;
    let sin = (::std::f64::consts::PI * tau).sin();

    sin * sin
}

fn windows<'s>(source: &'s [f64], size: usize, hop: usize) -> Windows<'s> {
    Windows {
        source, size, hop,
        pos: 0,
    }
}

fn dot_prod(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right)
        .fold(0.0, |sum, (l, r)| sum + l*r)
}

fn correlate(big: &[f64], small: &[f64]) -> usize {
    let steps = big.len() - small.len();
    let len = small.len();

    let mut norm = dot_prod(&big[..len], &big[..len]);
    let dot = dot_prod(big, small);
    let mut maxima = (0, dot * dot.abs() / norm);

    for i in 1..steps {
        let dot = dot_prod(&big[i..], small);
        norm = norm - big[i - 1].powi(2) + big[len - 1 + i].powi(2);
        let corr = dot * dot.abs() / norm;

        if corr > maxima.1 {
            maxima = (i, corr);
        }
    }

    maxima.0
}

fn ola(output: &mut [f64], frame: &[f64]) {
    assert_eq!(output.len(), frame.len());

    let len = output.len();
    for (i, (out, v)) in output.iter_mut().zip(frame).enumerate() {
        let tau = i as f64 / (len - 1) as f64;
        let sin = ( 0.5 * ::std::f64::consts::PI * tau ).sin();
        let beta = sin * sin;

        *out = (1.0 - beta) * (*out) + beta * (*v);
    }
}

struct Correlator {
    big_buf: Vec<f64>,
    small_buf: Vec<f64>,
}

impl Correlator {
    fn new() -> Self {
        Self {
            big_buf: vec![],
            small_buf: vec![],
        }
    }
    fn correlate(&mut self, big: &[f64], small: &[f64]) -> usize {
        let small_len = small.len();

        for i in 0..big.len() / 2 {
            self.big_buf.push(0.5 * (big[2 * i] + big[2*i + 1]))
        }
        for i in 0..small.len() / 2 {
            self.small_buf.push(0.5 * (small[2 * i] + small[2*i + 1]))
        }

        let pos = 2 * correlate(&self.big_buf, &self.small_buf);
        let d_pos = correlate(&big[pos .. pos + small_len + 1], small);

        self.big_buf.clear();
        self.small_buf.clear();

        pos + d_pos
    }
}

pub struct Sola {
    correlator: Correlator,
    sample_rate: usize,
}

impl Sola {
    pub fn new() -> Self {
        Sola {
            correlator: Correlator::new(),
            sample_rate: 44100,
        }
    }
    pub fn process(&mut self, input: &[f64], alpha: f64, freq: f64) -> Vec<f64> {
        // TODO: sample rate
        let win_size =
            if freq > 430.0 { 1024 }
            else { 2048 };
        let hop_s = win_size / 2;
        let hop_a = (hop_s as f64 / alpha).round() as usize;

        let d_hop = (1.5 * self.sample_rate as f64 / freq) as usize;

        let mut output = input[0..win_size].to_owned();
        output.reserve((input.len() as f64 * alpha).ceil() as _);

        let n = ((input.len() - win_size) as f64 / hop_a as f64).ceil() as usize - 1;

        for i in 1..n {
            let pos_a = hop_a * i;
            let pos_s = output.len() - hop_s;
            let win = &input[pos_a .. pos_a + win_size];

            let d_pos = self.correlator.correlate(&output[pos_s - d_hop ..], &win[..hop_s - d_hop]);

            let pos_a = pos_a + d_hop - d_pos;
            let win = &input[pos_a .. pos_a + win_size]; // this crashes sometimes
            ola(&mut output[pos_s..], &win[..hop_s]);

            output.extend(&win[hop_s..]);
        }

        output
    }
}
