//! JavaScript's `Math` functions, bit for bit as V8 computes them, so the
//! port rounds exactly as CesiumJS does. The platform's `sin`, `cos` and
//! `log` round differently in about one case in thirty, which is enough to
//! flip comparisons such as the extreme-point search in
//! `BoundingSphere.fromPoints`.
//!
//! `sin`, `cos` and `log` are ports of fdlibm 5.3 as V8's `base::ieee754`
//! has them. The rest come from the `libm` crate, whose results match V8's
//! (checked on 200,000 inputs each).

/// V8's `Math` on `f64`. Import the trait to call these as methods.
pub trait JsMath {
    fn js_sin(self) -> f64;
    fn js_cos(self) -> f64;
    /// `(Math.sin(x), Math.cos(x))`.
    fn js_sin_cos(self) -> (f64, f64);
    fn js_tan(self) -> f64;
    fn js_asin(self) -> f64;
    fn js_acos(self) -> f64;
    fn js_atan(self) -> f64;
    /// `Math.atan2(self, x)`.
    fn js_atan2(self, x: f64) -> f64;
    fn js_exp(self) -> f64;
    /// `Math.log`.
    fn js_ln(self) -> f64;
    fn js_cosh(self) -> f64;
}

impl JsMath for f64 {
    fn js_sin(self) -> f64 {
        sin(self)
    }
    fn js_cos(self) -> f64 {
        cos(self)
    }
    fn js_sin_cos(self) -> (f64, f64) {
        (sin(self), cos(self))
    }
    fn js_tan(self) -> f64 {
        libm::tan(self)
    }
    fn js_asin(self) -> f64 {
        libm::asin(self)
    }
    fn js_acos(self) -> f64 {
        libm::acos(self)
    }
    fn js_atan(self) -> f64 {
        libm::atan(self)
    }
    fn js_atan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }
    fn js_exp(self) -> f64 {
        libm::exp(self)
    }
    fn js_ln(self) -> f64 {
        log(self)
    }
    fn js_cosh(self) -> f64 {
        libm::cosh(self)
    }
}

// ---- fdlibm 5.3, as in V8's base/ieee754.cc ---------------------------------
// Kept close to the C so it can be checked line by line against fdlibm.
#[allow(clippy::needless_range_loop, clippy::explicit_counter_loop, clippy::approx_constant, clippy::eq_op)]
mod fdlibm {

    fn high(x: f64) -> i32 {
        (x.to_bits() >> 32) as i32
    }
    fn low(x: f64) -> u32 {
        x.to_bits() as u32
    }
    fn from_words(hi: i32, lo: u32) -> f64 {
        f64::from_bits(((hi as u32 as u64) << 32) | lo as u64)
    }

    fn scalbn(x: f64, n: i32) -> f64 {
        libm::scalbn(x, n)
    }

    const TWO_OVER_PI: [i32; 66] = [
        0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163, 0xABDEBB, 0xC561B7,
        0x246E3A, 0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129, 0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C,
        0x7026B4, 0x5F7E41, 0x3991D6, 0x398353, 0x39F49C, 0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11,
        0x8B5A0A, 0x6D1F6D, 0x367ECF, 0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7,
        0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3, 0x91615E,
        0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA, 0x73A8C9, 0x60E27B, 0xC08C6B,
    ];
    const NPIO2_HW: [i32; 32] = [
        0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB, 0x402C463A,
        0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A,
        0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC,
        0x4045FDBB, 0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
    ];
    const PIO2: [f64; 8] = [
        1.57079625129699707031e+00,
        7.54978941586159635335e-08,
        5.39030252995776476554e-15,
        3.28200341580791294123e-22,
        1.27065575308067607349e-29,
        1.22933308981111328932e-36,
        2.73370053816464559624e-44,
        2.16741683877804819444e-51,
    ];
    const TWO24: f64 = 1.67772160000000000000e+07;
    const TWON24: f64 = 5.96046447753906250000e-08;

    fn kernel_rem_pio2(x: &[f64], y: &mut [f64; 2], e0: i32, nx: usize) -> i32 {
        let prec = 2;
        let jk: usize = [2, 3, 4, 6][prec];
        let jp = jk;
        let jx = nx as i32 - 1;
        let mut jv = (e0 - 3) / 24;
        if jv < 0 {
            jv = 0;
        }
        let mut q0 = e0 - 24 * (jv + 1);
        let mut f = [0f64; 20];
        let mut q = [0f64; 20];
        let mut fq = [0f64; 20];
        let mut iq = [0i32; 20];
        let mut j = jv - jx;
        let m = jx + jk as i32;
        for i in 0..=m as usize {
            f[i] = if j < 0 { 0.0 } else { TWO_OVER_PI[j as usize] as f64 };
            j += 1;
        }
        for i in 0..=jk {
            let mut fw = 0.0;
            for j in 0..=jx as usize {
                fw += x[j] * f[jx as usize + i - j];
            }
            q[i] = fw;
        }
        let mut jz = jk;
        let (mut z, mut n, mut ih);
        loop {
            let mut i = 0;
            let mut j = jz;
            z = q[jz];
            while j > 0 {
                let fw = ((TWON24 * z) as i32) as f64;
                iq[i] = (z - TWO24 * fw) as i32;
                z = q[j - 1] + fw;
                i += 1;
                j -= 1;
            }
            z = scalbn(z, q0);
            z -= 8.0 * (z * 0.125).floor();
            n = z as i32;
            z -= n as f64;
            ih = 0;
            if q0 > 0 {
                let i = iq[jz - 1] >> (24 - q0);
                n += i;
                iq[jz - 1] -= i << (24 - q0);
                ih = iq[jz - 1] >> (23 - q0);
            } else if q0 == 0 {
                ih = iq[jz - 1] >> 23;
            } else if z >= 0.5 {
                ih = 2;
            }
            if ih > 0 {
                n += 1;
                let mut carry = 0;
                for i in 0..jz {
                    let j = iq[i];
                    if carry == 0 {
                        if j != 0 {
                            carry = 1;
                            iq[i] = 0x1000000 - j;
                        }
                    } else {
                        iq[i] = 0xffffff - j;
                    }
                }
                if q0 > 0 {
                    match q0 {
                        1 => iq[jz - 1] &= 0x7fffff,
                        2 => iq[jz - 1] &= 0x3fffff,
                        _ => {}
                    }
                }
                if ih == 2 {
                    z = 1.0 - z;
                    if carry != 0 {
                        z -= scalbn(1.0, q0);
                    }
                }
            }
            if z == 0.0 {
                let mut j = 0;
                let mut i = jz as i32 - 1;
                while i >= jk as i32 {
                    j |= iq[i as usize];
                    i -= 1;
                }
                if j == 0 {
                    let mut k = 1;
                    while jk >= k && iq[jk - k] == 0 {
                        k += 1;
                    }
                    for i in jz + 1..=jz + k {
                        f[jx as usize + i] = TWO_OVER_PI[(jv as usize) + i] as f64;
                        let mut fw = 0.0;
                        for j in 0..=jx as usize {
                            fw += x[j] * f[jx as usize + i - j];
                        }
                        q[i] = fw;
                    }
                    jz += k;
                    continue;
                }
            }
            break;
        }
        if z == 0.0 {
            jz -= 1;
            q0 -= 24;
            while iq[jz] == 0 {
                jz -= 1;
                q0 -= 24;
            }
        } else {
            z = scalbn(z, -q0);
            if z >= TWO24 {
                let fw = ((TWON24 * z) as i32) as f64;
                iq[jz] = (z - TWO24 * fw) as i32;
                jz += 1;
                q0 += 24;
                iq[jz] = fw as i32;
            } else {
                iq[jz] = z as i32;
            }
        }
        let mut fw = scalbn(1.0, q0);
        let mut i = jz as i32;
        while i >= 0 {
            q[i as usize] = fw * iq[i as usize] as f64;
            fw *= TWON24;
            i -= 1;
        }
        let mut i = jz as i32;
        while i >= 0 {
            let mut fw = 0.0;
            let mut k = 0;
            while k <= jp && k as i32 <= jz as i32 - i {
                fw += PIO2[k] * q[i as usize + k];
                k += 1;
            }
            fq[jz - i as usize] = fw;
            i -= 1;
        }
        let mut fw = 0.0;
        let mut i = jz as i32;
        while i >= 0 {
            fw += fq[i as usize];
            i -= 1;
        }
        y[0] = if ih == 0 { fw } else { -fw };
        fw = fq[0] - fw;
        for i in 1..=jz {
            fw += fq[i];
        }
        y[1] = if ih == 0 { fw } else { -fw };
        n & 7
    }

    fn rem_pio2(x: f64, y: &mut [f64; 2]) -> i32 {
        const INVPIO2: f64 = 6.36619772367581382433e-01;
        const PIO2_1: f64 = 1.57079632673412561417e+00;
        const PIO2_1T: f64 = 6.07710050650619224932e-11;
        const PIO2_2: f64 = 6.07710050630396597660e-11;
        const PIO2_2T: f64 = 2.02226624879595063154e-21;
        const PIO2_3: f64 = 2.02226624871116645580e-21;
        const PIO2_3T: f64 = 8.47842766036889956997e-32;
        let hx = high(x);
        let ix = hx & 0x7fffffff;
        if ix <= 0x3fe921fb {
            y[0] = x;
            y[1] = 0.0;
            return 0;
        }
        if ix < 0x4002d97c {
            if hx > 0 {
                let mut z = x - PIO2_1;
                if ix != 0x3ff921fb {
                    y[0] = z - PIO2_1T;
                    y[1] = (z - y[0]) - PIO2_1T;
                } else {
                    z -= PIO2_2;
                    y[0] = z - PIO2_2T;
                    y[1] = (z - y[0]) - PIO2_2T;
                }
                return 1;
            } else {
                let mut z = x + PIO2_1;
                if ix != 0x3ff921fb {
                    y[0] = z + PIO2_1T;
                    y[1] = (z - y[0]) + PIO2_1T;
                } else {
                    z += PIO2_2;
                    y[0] = z + PIO2_2T;
                    y[1] = (z - y[0]) + PIO2_2T;
                }
                return -1;
            }
        }
        if ix <= 0x413921fb {
            let t = x.abs();
            let n = (t * INVPIO2 + 0.5) as i32;
            let fn_ = n as f64;
            let mut r = t - fn_ * PIO2_1;
            let mut w = fn_ * PIO2_1T;
            if n < 32 && ix != NPIO2_HW[(n - 1) as usize] {
                y[0] = r - w;
            } else {
                let j = ix >> 20;
                y[0] = r - w;
                let mut i = j - ((high(y[0]) >> 20) & 0x7ff);
                if i > 16 {
                    let t = r;
                    w = fn_ * PIO2_2;
                    r = t - w;
                    w = fn_ * PIO2_2T - ((t - r) - w);
                    y[0] = r - w;
                    i = j - ((high(y[0]) >> 20) & 0x7ff);
                    if i > 49 {
                        let t = r;
                        w = fn_ * PIO2_3;
                        r = t - w;
                        w = fn_ * PIO2_3T - ((t - r) - w);
                        y[0] = r - w;
                    }
                }
            }
            y[1] = (r - y[0]) - w;
            if hx < 0 {
                y[0] = -y[0];
                y[1] = -y[1];
                return -n;
            }
            return n;
        }
        if ix >= 0x7ff00000 {
            y[0] = x - x;
            y[1] = y[0];
            return 0;
        }
        let e0 = (ix >> 20) - 1046;
        let mut z = from_words(ix - (e0 << 20), low(x));
        let mut tx = [0f64; 3];
        for t in tx.iter_mut().take(2) {
            *t = (z as i32) as f64;
            z = (z - *t) * TWO24;
        }
        tx[2] = z;
        let mut nx = 3;
        while tx[nx - 1] == 0.0 {
            nx -= 1;
        }
        let n = kernel_rem_pio2(&tx, y, e0, nx);
        if hx < 0 {
            y[0] = -y[0];
            y[1] = -y[1];
            return -n;
        }
        n
    }

    fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
        const S1: f64 = -1.66666666666666324348e-01;
        const S2: f64 = 8.33333333332248946124e-03;
        const S3: f64 = -1.98412698298579493134e-04;
        const S4: f64 = 2.75573137070700676789e-06;
        const S5: f64 = -2.50507602534068634195e-08;
        const S6: f64 = 1.58969099521155010221e-10;
        let ix = high(x) & 0x7fffffff;
        if ix < 0x3e400000 && (x as i32) == 0 {
            return x;
        }
        let z = x * x;
        let v = z * x;
        let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
        if iy == 0 {
            x + v * (S1 + z * r)
        } else {
            x - ((z * (0.5 * y - v * r) - y) - v * S1)
        }
    }

    fn kernel_cos(x: f64, y: f64) -> f64 {
        const C1: f64 = 4.16666666666666019037e-02;
        const C2: f64 = -1.38888888888741095749e-03;
        const C3: f64 = 2.48015872894767294178e-05;
        const C4: f64 = -2.75573143513906633035e-07;
        const C5: f64 = 2.08757232129817482790e-09;
        const C6: f64 = -1.13596475577881948265e-11;
        let ix = high(x) & 0x7fffffff;
        if ix < 0x3e400000 && (x as i32) == 0 {
            return 1.0;
        }
        let z = x * x;
        let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
        if ix < 0x3fd33333 {
            1.0 - (0.5 * z - (z * r - x * y))
        } else {
            let qx = if ix > 0x3fe90000 { 0.28125 } else { from_words(ix - 0x00200000, 0) };
            let hz = 0.5 * z - qx;
            let a = 1.0 - qx;
            a - (hz - (z * r - x * y))
        }
    }

    pub(crate) fn sin(x: f64) -> f64 {
        let ix = high(x) & 0x7fffffff;
        if ix <= 0x3fe921fb {
            return kernel_sin(x, 0.0, 0);
        }
        if ix >= 0x7ff00000 {
            return x - x;
        }
        let mut y = [0.0; 2];
        match rem_pio2(x, &mut y) & 3 {
            0 => kernel_sin(y[0], y[1], 1),
            1 => kernel_cos(y[0], y[1]),
            2 => -kernel_sin(y[0], y[1], 1),
            _ => -kernel_cos(y[0], y[1]),
        }
    }

    pub(crate) fn cos(x: f64) -> f64 {
        let ix = high(x) & 0x7fffffff;
        if ix <= 0x3fe921fb {
            return kernel_cos(x, 0.0);
        }
        if ix >= 0x7ff00000 {
            return x - x;
        }
        let mut y = [0.0; 2];
        match rem_pio2(x, &mut y) & 3 {
            0 => kernel_cos(y[0], y[1]),
            1 => -kernel_sin(y[0], y[1], 1),
            2 => -kernel_cos(y[0], y[1]),
            _ => kernel_sin(y[0], y[1], 1),
        }
    }

    pub(crate) fn log(x: f64) -> f64 {
        const LN2_HI: f64 = 6.93147180369123816490e-01;
        const LN2_LO: f64 = 1.90821492927058770002e-10;
        const TWO54: f64 = 1.80143985094819840000e+16;
        const LG1: f64 = 6.666666666666735130e-01;
        const LG2: f64 = 3.999999999940941908e-01;
        const LG3: f64 = 2.857142874366239149e-01;
        const LG4: f64 = 2.222219843214978396e-01;
        const LG5: f64 = 1.818357216161805012e-01;
        const LG6: f64 = 1.531383769920937332e-01;
        const LG7: f64 = 1.479819860511658591e-01;
        let mut x = x;
        let mut hx = high(x);
        let lx = low(x);
        let mut k = 0;
        if hx < 0x00100000 {
            if ((hx & 0x7fffffff) as u32 | lx) == 0 {
                return f64::NEG_INFINITY;
            }
            if hx < 0 {
                return f64::NAN;
            }
            k -= 54;
            x *= TWO54;
            hx = high(x);
        }
        if hx >= 0x7ff00000 {
            return x + x;
        }
        k += (hx >> 20) - 1023;
        hx &= 0x000fffff;
        let i = (hx + 0x95f64) & 0x100000;
        x = from_words(hx | (i ^ 0x3ff00000), low(x));
        k += i >> 20;
        let f = x - 1.0;
        if (0x000fffff & (2 + hx)) < 3 {
            if f == 0.0 {
                if k == 0 {
                    return 0.0;
                }
                let dk = k as f64;
                return dk * LN2_HI + dk * LN2_LO;
            }
            let r = f * f * (0.5 - 0.33333333333333333 * f);
            if k == 0 {
                return f - r;
            }
            let dk = k as f64;
            return dk * LN2_HI - ((r - dk * LN2_LO) - f);
        }
        let s = f / (2.0 + f);
        let dk = k as f64;
        let z = s * s;
        let mut i = hx - 0x6147a;
        let w = z * z;
        let j = 0x6b851 - hx;
        let t1 = w * (LG2 + w * (LG4 + w * LG6));
        let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
        i |= j;
        let r = t2 + t1;
        if i > 0 {
            let hfsq = 0.5 * f * f;
            if k == 0 {
                f - (hfsq - s * (hfsq + r))
            } else {
                dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
            }
        } else if k == 0 {
            f - s * (f - r)
        } else {
            dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
        }
    }
}

use fdlibm::{cos, log, sin};
