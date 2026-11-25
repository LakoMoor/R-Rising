//! `ProjectM.Motion2DEasings` leaf easing curves (double precision), arguments `(t, b, c, d)` =
//! (time, start, change, duration). Operation order follows the original instructions; verified
//! against `evidence/oracles/easings.tsv`. Sine, expo, circ and elastic are not ported yet.

pub fn linear(t: f64, b: f64, c: f64, d: f64) -> f64 {
    (t / d) * c + b
}
pub fn in_quad(t: f64, b: f64, c: f64, d: f64) -> f64 {
    (t / d) * c * (t / d) + b
}
pub fn out_quad(t: f64, b: f64, c: f64, d: f64) -> f64 {
    (-c) * (t / d) * (t / d - 2.0) + b
}
pub fn in_out_quad(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / (d * 0.5);
    if 1.0 <= p {
        let q = p - 1.0;
        ((q - 2.0) * q - 1.0) * (-c) * 0.5 + b
    } else {
        c * 0.5 * p * p + b
    }
}
pub fn in_cubic(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / d;
    p * c * p * p + b
}
pub fn out_cubic(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let q = t / d - 1.0;
    (q * q * q + 1.0) * c + b
}
pub fn in_out_cubic(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / (d * 0.5);
    if 1.0 <= p {
        let q = p - 2.0;
        (q * q * q + 2.0) * c * 0.5 + b
    } else {
        c * 0.5 * p * p * p + b
    }
}
pub fn in_quart(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / d;
    p * c * p * p * p + b
}
pub fn out_quart(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let q = t / d - 1.0;
    (q * q * q * q - 1.0) * (-c) + b
}
pub fn in_out_quart(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / (d * 0.5);
    if 1.0 <= p {
        let q = p - 2.0;
        (q * q * q * q - 2.0) * (-c) * 0.5 + b
    } else {
        c * 0.5 * p * p * p * p + b
    }
}
pub fn in_quint(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / d;
    p * c * p * p * p * p + b
}
pub fn out_quint(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let q = t / d - 1.0;
    (q * q * q * q * q + 1.0) * c + b
}
pub fn in_out_quint(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / (d * 0.5);
    if 1.0 <= p {
        let q = p - 2.0;
        (q * q * q * q * q + 2.0) * c * 0.5 + b
    } else {
        c * 0.5 * p * p * p * p * p + b
    }
}
pub fn in_back(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / d;
    p * c * p * (p * 2.70158 - 1.70158) + b
}
pub fn out_back(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let q = t / d - 1.0;
    ((q * 2.70158 + 1.70158) * (q * q) + 1.0) * c + b
}
pub fn in_out_back(t: f64, b: f64, c: f64, d: f64) -> f64 {
    // The original halves `c` and `d` first and squares before multiplying.
    let half_c = c * 0.5;
    let p = t / (d * 0.5);
    if 1.0 <= p {
        let q = p - 2.0;
        ((q * 3.5949095 + 2.5949095) * (q * q) + 2.0) * half_c + b
    } else {
        (p * 3.5949095 - 2.5949095) * (p * p) * half_c + b
    }
}
pub fn out_bounce(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let p = t / d;
    if p < 0.36363636363636365 {
        p * 7.5625 * p * c + b
    } else if 0.7272727272727273 <= p {
        if 0.9090909090909091 <= p {
            let q = p - 0.9545454545454546;
            (q * 7.5625 * q + 0.984375) * c + b
        } else {
            let q = p - 0.8181818181818182;
            (q * 7.5625 * q + 0.9375) * c + b
        }
    } else {
        let q = p - 0.5454545454545454;
        (q * 7.5625 * q + 0.75) * c + b
    }
}
pub fn in_bounce(t: f64, b: f64, c: f64, d: f64) -> f64 {
    (c - out_bounce(d - t, 0.0, c, d)) + b
}
pub fn in_out_bounce(t: f64, b: f64, c: f64, d: f64) -> f64 {
    let v = if t < d * 0.5 {
        ((c - out_bounce(d - (t + t), 0.0, c, d)) + 0.0) * 0.5
    } else {
        out_bounce((t + t) - d, 0.0, c, d) * 0.5 + c * 0.5
    };
    v + b
}
