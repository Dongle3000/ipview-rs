//! Summary statistics over a set of tickets.

use std::collections::BTreeMap;

use crate::ticket::Ticket;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub mean: f64,
    pub min: f64,
    pub p5: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

impl Summary {
    pub fn of(mut v: Vec<f64>) -> Option<Summary> {
        if v.is_empty() {
            return None;
        }
        v.sort_by(f64::total_cmp);
        let pct = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        Some(Summary {
            count: v.len(),
            mean: v.iter().sum::<f64>() / v.len() as f64,
            min: v[0],
            p5: pct(0.05),
            median: pct(0.5),
            p95: pct(0.95),
            max: v[v.len() - 1],
        })
    }
}

/// MOS bands, best first: (lower bound inclusive, upper bound exclusive, label).
pub const MOS_BANDS: [(f64, f64, &str); 5] = [
    (4.3, f64::INFINITY, ">= 4.3"),
    (4.0, 4.3, "4.0 - 4.3"),
    (3.6, 4.0, "3.6 - 4.0"),
    (3.1, 3.6, "3.1 - 3.6"),
    (f64::NEG_INFINITY, 3.1, "< 3.1"),
];

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub tickets: usize,
    /// Earliest and latest end-of-communication time (Unix seconds).
    pub period: Option<(i64, i64)>,
    pub mos: Option<Summary>,
    pub loss_pct: Option<Summary>,
    pub rtd: Option<Summary>,
    pub jitter_delay: Option<Summary>,
    /// Ticket counts per entry of [`MOS_BANDS`].
    pub mos_bands: [usize; 5],
    pub codecs: BTreeMap<String, usize>,
    pub equipment: BTreeMap<String, usize>,
}

impl Stats {
    pub fn compute<'a>(tickets: impl IntoIterator<Item = &'a Ticket>) -> Stats {
        let tickets: Vec<&Ticket> = tickets.into_iter().collect();
        let metrics: Vec<_> = tickets.iter().map(|t| t.metrics()).collect();
        let times: Vec<i64> = tickets.iter().filter_map(|t| t.end_time()).collect();
        let mos: Vec<f64> = metrics.iter().filter_map(|m| m.mos).collect();

        let mut mos_bands = [0; 5];
        for m in &mos {
            if let Some(i) = MOS_BANDS.iter().position(|&(lo, hi, _)| *m >= lo && *m < hi) {
                mos_bands[i] += 1;
            }
        }
        let count_by = |id: u16| {
            let mut counts = BTreeMap::<String, usize>::new();
            for t in &tickets {
                *counts.entry(t.value(id).map(|v| v.to_string()).unwrap_or_else(|| "-".into())).or_default() += 1;
            }
            counts
        };

        Stats {
            tickets: tickets.len(),
            period: times.iter().min().zip(times.iter().max()).map(|(a, b)| (*a, *b)),
            loss_pct: Summary::of(metrics.iter().filter_map(|m| m.loss_pct).collect()),
            rtd: Summary::of(metrics.iter().filter_map(|m| m.rtd).collect()),
            jitter_delay: Summary::of(metrics.iter().filter_map(|m| m.jitter_delay).collect()),
            mos: Summary::of(mos),
            mos_bands,
            codecs: count_by(15),
            equipment: count_by(6),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::tests::tlv;

    #[test]
    fn summary_percentiles() {
        let s = Summary::of((1..=101).map(f64::from).collect()).unwrap();
        assert_eq!((s.min, s.p5, s.median, s.p95, s.max, s.mean), (1.0, 6.0, 51.0, 96.0, 101.0, 51.0));
        assert!(Summary::of(vec![]).is_none());
    }

    #[test]
    fn counts_bands_and_codecs() {
        let good = Ticket::from_raw(&tlv(&[(1, &[0, 0, 0, 5]), (15, &[0]), (20, &[20]), (22, &[0, 0, 1, 0])]));
        let opus = Ticket::from_raw(&tlv(&[(1, &[0, 0, 0, 9]), (15, &[8])]));
        let s = Stats::compute([&good, &opus]);
        assert_eq!(s.tickets, 2);
        assert_eq!(s.period, Some((5, 9)));
        assert_eq!(s.mos_bands, [1, 0, 0, 0, 0]);
        assert_eq!(s.codecs.get("Opus"), Some(&1));
    }
}
