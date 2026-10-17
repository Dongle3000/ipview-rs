//! ITU-T G.107 E-model, as implemented by IPView 2.0.1.
//!
//! The formulas are the standard ones; [`Params::default`] holds the
//! constructor defaults found in the original, which match the G.107 default
//! values except where noted.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Send loudness rating (dB).
    pub slr: f64,
    /// Receive loudness rating (dB).
    pub rlr: f64,
    /// Sidetone masking rating (dB).
    pub stmr: f64,
    /// Listener sidetone rating (dB).
    pub lstr: f64,
    /// D-value of telephone, send side.
    pub ds: f64,
    /// D-value of telephone, receive side (unused by the formulas, kept for completeness).
    pub dr: f64,
    /// Talker echo loudness rating (dB).
    pub telr: f64,
    /// Weighted echo path loss (dB).
    pub wepl: f64,
    /// Mean one-way delay of the echo path (ms).
    pub t: f64,
    /// Round-trip delay in a 4-wire loop (ms).
    pub tr: f64,
    /// Absolute delay in echo-free connections (ms).
    pub ta: f64,
    /// Number of quantization distortion units.
    pub qdu: f64,
    /// Equipment impairment factor.
    pub ie: f64,
    /// Packet-loss robustness factor.
    pub bpl: f64,
    /// Random packet-loss probability (%).
    pub ppl: f64,
    /// Burst ratio.
    pub burst_r: f64,
    /// Circuit noise referred to the 0 dBr point (dBm0p).
    pub nc: f64,
    /// Noise floor at the receive side (dBmp).
    pub nfor: f64,
    /// Room noise at the send side (dB(A)).
    pub ps: f64,
    /// Room noise at the receive side (dB(A)).
    pub pr: f64,
    /// Advantage factor.
    pub a: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            slr: 8.0,
            rlr: 2.0,
            stmr: 15.0,
            // G.107 uses 18; IPView's constructor leaves this at 0 and the MOS
            // calculation always sets it explicitly.
            lstr: 18.0,
            ds: 3.0,
            dr: 3.0,
            telr: 65.0,
            wepl: 110.0,
            t: 0.0,
            tr: 0.0,
            ta: 0.0,
            qdu: 1.0,
            ie: 0.0,
            bpl: 1.0,
            ppl: 0.0,
            burst_r: 1.0,
            nc: -70.0,
            nfor: -64.0,
            ps: 35.0,
            pr: 35.0,
            a: 0.0,
        }
    }
}

impl Params {
    /// Total noise power No (dBm0p).
    fn no(&self) -> f64 {
        let nfo = self.nfor + self.rlr;
        let pre = self.pr + 10.0 * (1.0 + 10f64.powf((10.0 - self.lstr) / 10.0)).log10();
        let nor = self.rlr - 121.0 + pre + 0.008 * (pre - 35.0).powi(2);
        let olr = self.slr + self.rlr;
        let nos = self.ps - self.slr - self.ds - 100.0 + 0.004 * (self.ps - olr - self.ds - 14.0).powi(2);
        10.0 * (10f64.powf(self.nc / 10.0) + 10f64.powf(nos / 10.0) + 10f64.powf(nor / 10.0) + 10f64.powf(nfo / 10.0))
            .log10()
    }

    /// Basic signal-to-noise ratio Ro.
    fn ro(&self) -> f64 {
        15.0 - 1.5 * (self.slr + self.no())
    }

    fn iolr(&self) -> f64 {
        let xolr = self.slr + self.rlr + 0.2 * (64.0 + self.no() - self.rlr);
        20.0 * ((1.0 + (xolr / 8.0).powi(8)).powf(1.0 / 8.0) - xolr / 8.0)
    }

    fn stmro(&self) -> f64 {
        -10.0 * (10f64.powf(-self.stmr / 10.0) + (-self.t / 4.0).exp() * 10f64.powf(-self.telr / 10.0)).log10()
    }

    fn ist(&self) -> f64 {
        let s = self.stmro();
        12.0 * (1.0 + ((s - 13.0) / 6.0).powi(8)).powf(1.0 / 8.0)
            - 28.0 * (1.0 + ((s + 1.0) / 19.4).powi(35)).powf(1.0 / 35.0)
            - 13.0 * (1.0 + ((s - 3.0) / 33.0).powi(13)).powf(1.0 / 13.0)
            + 29.0
    }

    fn iq(&self) -> f64 {
        let qdu = self.qdu.max(1.0);
        let q = 37.0 - 15.0 * qdu.log10();
        let g = 1.07 + 0.258 * q + 0.0602 * q * q;
        let y = (self.ro() - 100.0) / 15.0 + 46.0 / 8.4 - g / 9.0;
        let z = 46.0 / 30.0 - g / 40.0;
        15.0 * (1.0 + 10f64.powf(y) + 10f64.powf(z)).log10()
    }

    /// Simultaneous impairment factor Is.
    fn is(&self) -> f64 {
        self.iolr() + self.ist() + self.iq()
    }

    fn idte(&self) -> f64 {
        let mut terv = self.telr - 40.0 * ((1.0 + self.t / 10.0) / (1.0 + self.t / 150.0)).log10()
            + 6.0 * (-0.3 * self.t * self.t).exp();
        if self.stmr < 9.0 {
            terv += self.ist() / 2.0;
        }
        let re = 80.0 + 2.5 * (terv - 14.0);
        let roe = -1.5 * (self.no() - self.rlr);
        let d = roe - re;
        let mut idte = (d / 2.0 + (d * d / 4.0 + 100.0).sqrt() - 1.0) * (1.0 - (-self.t).exp());
        if self.stmr > 20.0 {
            idte = (idte * idte + self.ist().powi(2)).sqrt();
        }
        idte
    }

    fn idle(&self) -> f64 {
        let rle = 10.5 * (self.wepl + 7.0) * (self.tr + 1.0).powf(-0.25);
        let d = self.ro() - rle;
        d / 2.0 + (d * d / 4.0 + 169.0).sqrt()
    }

    fn idd(&self) -> f64 {
        if self.ta <= 100.0 {
            return 0.0;
        }
        let x = (self.ta / 100.0).log10() / 2f64.log10();
        25.0 * ((1.0 + x.powi(6)).powf(1.0 / 6.0) - 3.0 * (1.0 + (x / 3.0).powi(6)).powf(1.0 / 6.0) + 2.0)
    }

    /// Delay impairment factor Id.
    fn id(&self) -> f64 {
        self.idte() + self.idle() + self.idd()
    }

    /// Effective equipment impairment factor Ie-eff.
    fn ie_eff(&self) -> f64 {
        self.ie + (95.0 - self.ie) * self.ppl / (self.ppl / self.burst_r + self.bpl)
    }

    /// Transmission rating factor R.
    pub fn r(&self) -> f64 {
        self.ro() - self.is() - self.id() - self.ie_eff() + self.a
    }

    /// Estimated mean opinion score (1.0..=4.5).
    pub fn mos(&self) -> f64 {
        r_to_mos(self.r())
    }
}

pub fn r_to_mos(r: f64) -> f64 {
    let mos = if r < 0.0 {
        1.0
    } else if r > 100.0 {
        4.5
    } else {
        1.0 + 0.035 * r + r * (r - 60.0) * (100.0 - r) * 7e-6
    };
    mos.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g107_defaults_give_r_93_2() {
        let r = Params::default().r();
        assert!((r - 93.2).abs() < 0.05, "R = {r}");
    }

    #[test]
    fn mos_mapping_endpoints() {
        assert_eq!(r_to_mos(-5.0), 1.0);
        assert_eq!(r_to_mos(120.0), 4.5);
        assert!((r_to_mos(93.2) - 4.41).abs() < 0.01);
    }

    #[test]
    fn delay_and_loss_lower_r() {
        let base = Params::default();
        let delayed = Params { ta: 300.0, ..base };
        let lossy = Params { ppl: 5.0, ie: 0.0, bpl: 25.1, ..base };
        assert!(delayed.r() < base.r());
        assert!(lossy.r() < base.r());
    }
}
