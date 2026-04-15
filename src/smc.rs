use crate::types::{Bias, Candle, SignalKind, SmcSignal};
use chrono::Utc;

pub struct SmcEngine;

impl SmcEngine {
    pub fn new() -> Self {
        Self
    }

    /// Run all SMC detectors and return signals sorted by strength desc.
    pub fn analyze(&self, candles: &[Candle]) -> Vec<SmcSignal> {
        if candles.len() < 10 {
            return vec![];
        }

        let mut signals = Vec::new();
        signals.extend(detect_fvg(candles));
        signals.extend(detect_order_blocks(candles));
        signals.extend(detect_bos_choch(candles));
        signals.extend(detect_liquidity_zones(candles));
        signals.extend(detect_smart_money_flow(candles));

        signals.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap_or(std::cmp::Ordering::Equal));
        signals
    }

    /// Derive overall bias from signals + short-term EMA relationship.
    pub fn overall_bias(&self, candles: &[Candle], signals: &[SmcSignal]) -> Bias {
        if candles.is_empty() {
            return Bias::Neutral;
        }

        let bull: usize = signals.iter().filter(|s| s.bias == Bias::Bullish).count();
        let bear: usize = signals.iter().filter(|s| s.bias == Bias::Bearish).count();

        // 20-candle simple mean as trend proxy
        let window = &candles[candles.len().saturating_sub(20)..];
        let mean: f64 = window.iter().map(|c| c.close).sum::<f64>() / window.len() as f64;
        let cur = candles.last().unwrap().close;

        let price_bullish = cur > mean * 1.005;
        let price_bearish = cur < mean * 0.995;

        match (price_bullish, price_bearish) {
            (true, _) if bull >= bear => Bias::Bullish,
            (_, true) if bear > bull  => Bias::Bearish,
            _ if bull > bear * 2      => Bias::Bullish,
            _ if bear > bull * 2      => Bias::Bearish,
            _                         => Bias::Neutral,
        }
    }
}

// ── Detectors ─────────────────────────────────────────────────────────────────

/// Fair Value Gap: 3-candle imbalance where candle[i-2] and candle[i] don't overlap.
fn detect_fvg(candles: &[Candle]) -> Vec<SmcSignal> {
    let mut out = Vec::new();
    let n = candles.len();

    for i in 2..n {
        let c0 = &candles[i - 2];
        let c2 = &candles[i];

        // Bullish FVG: c0.high < c2.low  → gap in between
        if c2.low > c0.high {
            let gap_pct = (c2.low - c0.high) / c0.high;
            if gap_pct >= 0.001 {
                out.push(SmcSignal {
                    kind: SignalKind::FairValueGap,
                    bias: Bias::Bullish,
                    price_level: (c0.high + c2.low) / 2.0,
                    strength: (gap_pct * 100.0).clamp(0.0, 1.0),
                    description: format!(
                        "Bullish FVG {:.4}%: zone {:.4}–{:.4} | price likely to retrace and fill gap before continuing up",
                        gap_pct * 100.0, c0.high, c2.low
                    ),
                    detected_at: Utc::now(),
                });
            }
        }

        // Bearish FVG: c2.high < c0.low  → gap in between
        if c2.high < c0.low {
            let gap_pct = (c0.low - c2.high) / c0.low;
            if gap_pct >= 0.001 {
                out.push(SmcSignal {
                    kind: SignalKind::FairValueGap,
                    bias: Bias::Bearish,
                    price_level: (c2.high + c0.low) / 2.0,
                    strength: (gap_pct * 100.0).clamp(0.0, 1.0),
                    description: format!(
                        "Bearish FVG {:.4}%: zone {:.4}–{:.4} | price likely to retrace and fill gap before continuing down",
                        gap_pct * 100.0, c2.high, c0.low
                    ),
                    detected_at: Utc::now(),
                });
            }
        }
    }

    out
}

/// Order Block: last opposing candle before a decisive structural move.
fn detect_order_blocks(candles: &[Candle]) -> Vec<SmcSignal> {
    let mut out = Vec::new();
    let n = candles.len();

    for i in 1..n.saturating_sub(2) {
        let ob = &candles[i];
        // We need at least two candles after OB to confirm the move
        let confirm = &candles[i + 2];

        // Bullish OB: bearish candle (close < open) followed by a bullish explosion above ob.high
        if ob.close < ob.open && confirm.close > ob.high * 1.003 {
            let body = (ob.open - ob.close) / ob.open;
            out.push(SmcSignal {
                kind: SignalKind::OrderBlock,
                bias: Bias::Bullish,
                price_level: (ob.open + ob.close) / 2.0,
                strength: (body * 50.0 + 0.5).clamp(0.0, 1.0),
                description: format!(
                    "Bullish OB: demand zone {:.4}–{:.4} | institutional buy orders placed here, high-probability long entry on retest",
                    ob.close, ob.open
                ),
                detected_at: Utc::now(),
            });
        }

        // Bearish OB: bullish candle (close > open) followed by a bearish explosion below ob.low
        if ob.close > ob.open && confirm.close < ob.low * 0.997 {
            let body = (ob.close - ob.open) / ob.open;
            out.push(SmcSignal {
                kind: SignalKind::OrderBlock,
                bias: Bias::Bearish,
                price_level: (ob.open + ob.close) / 2.0,
                strength: (body * 50.0 + 0.5).clamp(0.0, 1.0),
                description: format!(
                    "Bearish OB: supply zone {:.4}–{:.4} | institutional sell orders placed here, high-probability short entry on retest",
                    ob.open, ob.close
                ),
                detected_at: Utc::now(),
            });
        }
    }

    out
}

/// Break of Structure + Change of Character detection.
fn detect_bos_choch(candles: &[Candle]) -> Vec<SmcSignal> {
    let mut out = Vec::new();
    let n = candles.len();
    let lookback = 5usize;

    // BOS: close breaks previous N-candle swing high/low
    for i in lookback..n {
        let window = &candles[i - lookback..i];
        let cur = &candles[i];
        let prev = &candles[i - 1];

        let swing_high = window.iter().map(|c| c.high).fold(f64::NEG_INFINITY, f64::max);
        let swing_low  = window.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);

        if cur.close > swing_high && prev.close <= swing_high {
            out.push(SmcSignal {
                kind: SignalKind::BreakOfStructure,
                bias: Bias::Bullish,
                price_level: swing_high,
                strength: 0.75,
                description: format!(
                    "BOS Bullish: closed above {:.4} swing high | trend continuation signal, higher highs expected",
                    swing_high
                ),
                detected_at: Utc::now(),
            });
        }

        if cur.close < swing_low && prev.close >= swing_low {
            out.push(SmcSignal {
                kind: SignalKind::BreakOfStructure,
                bias: Bias::Bearish,
                price_level: swing_low,
                strength: 0.75,
                description: format!(
                    "BOS Bearish: closed below {:.4} swing low | trend continuation signal, lower lows expected",
                    swing_low
                ),
                detected_at: Utc::now(),
            });
        }
    }

    // CHoCH: compare first half vs second half of last 20 candles
    if n >= 20 {
        let first  = &candles[n - 20..n - 10];
        let second = &candles[n - 10..n];
        let last   = candles.last().unwrap();

        let f_high = first.iter().map(|c| c.high).fold(f64::NEG_INFINITY, f64::max);
        let f_low  = first.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
        let s_high = second.iter().map(|c| c.high).fold(f64::NEG_INFINITY, f64::max);
        let s_low  = second.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);

        // Downtrend (lower highs + lower lows) breaks above first half high → CHoCH bullish
        if s_high < f_high && s_low < f_low && last.close > f_high * 0.99 {
            out.push(SmcSignal {
                kind: SignalKind::ChangeOfCharacter,
                bias: Bias::Bullish,
                price_level: last.close,
                strength: 0.87,
                description: "CHoCH Bullish: prior downtrend reversing | first higher high forming, watch for confirmation and long entry on pullback".to_string(),
                detected_at: Utc::now(),
            });
        }

        // Uptrend (higher highs + higher lows) breaks below first half low → CHoCH bearish
        if s_high > f_high && s_low > f_low && last.close < f_low * 1.01 {
            out.push(SmcSignal {
                kind: SignalKind::ChangeOfCharacter,
                bias: Bias::Bearish,
                price_level: last.close,
                strength: 0.87,
                description: "CHoCH Bearish: prior uptrend reversing | first lower low forming, watch for confirmation and short entry on bounce".to_string(),
                detected_at: Utc::now(),
            });
        }
    }

    out
}

/// Equal highs / equal lows → pending liquidity pools likely to be swept.
fn detect_liquidity_zones(candles: &[Candle]) -> Vec<SmcSignal> {
    let mut out = Vec::new();
    let tolerance = 0.002; // 0.2% considered "equal"

    // Cluster equal highs (sell-side liquidity above)
    let clusters_high = cluster_levels(candles.iter().map(|c| c.high).collect(), tolerance);
    for (level, count) in &clusters_high {
        if *count >= 2 {
            out.push(SmcSignal {
                kind: SignalKind::LiquidityZone,
                bias: Bias::Bearish,
                price_level: *level,
                strength: (*count as f64 * 0.25).clamp(0.0, 1.0),
                description: format!(
                    "Sell-side liquidity at {:.4} ({count} equal highs) | stop orders clustered above, price likely to sweep before reversal",
                    level
                ),
                detected_at: Utc::now(),
            });
        }
    }

    // Cluster equal lows (buy-side liquidity below)
    let clusters_low = cluster_levels(candles.iter().map(|c| c.low).collect(), tolerance);
    for (level, count) in &clusters_low {
        if *count >= 2 {
            out.push(SmcSignal {
                kind: SignalKind::LiquidityZone,
                bias: Bias::Bullish,
                price_level: *level,
                strength: (*count as f64 * 0.25).clamp(0.0, 1.0),
                description: format!(
                    "Buy-side liquidity at {:.4} ({count} equal lows) | stop orders clustered below, price likely to sweep before reversal",
                    level
                ),
                detected_at: Utc::now(),
            });
        }
    }

    out
}

/// Volume-profile: detect institutional accumulation or distribution in last 10 candles.
fn detect_smart_money_flow(candles: &[Candle]) -> Vec<SmcSignal> {
    let mut out = Vec::new();
    let recent = &candles[candles.len().saturating_sub(10)..];
    if recent.is_empty() {
        return out;
    }

    let total_vol: f64 = recent.iter().map(|c| c.volume).sum();
    if total_vol == 0.0 {
        return out;
    }

    let bull_vol: f64 = recent.iter().filter(|c| c.close > c.open).map(|c| c.volume).sum();
    let bear_vol: f64 = recent.iter().filter(|c| c.close < c.open).map(|c| c.volume).sum();

    let bull_ratio = bull_vol / total_vol;
    let bear_ratio = bear_vol / total_vol;

    // Highest-volume candle is the anchor price level
    let anchor = recent
        .iter()
        .max_by(|a, b| a.volume.partial_cmp(&b.volume).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();

    if bull_ratio > 0.65 {
        out.push(SmcSignal {
            kind: SignalKind::SmartMoneyAccumulation,
            bias: Bias::Bullish,
            price_level: anchor.close,
            strength: bull_ratio,
            description: format!(
                "Smart Money Accumulation | {:.1}% of last-10 volume is buying | institutions absorbing supply — bullish continuation probable",
                bull_ratio * 100.0
            ),
            detected_at: Utc::now(),
        });
    } else if bear_ratio > 0.65 {
        out.push(SmcSignal {
            kind: SignalKind::SmartMoneyDistribution,
            bias: Bias::Bearish,
            price_level: anchor.close,
            strength: bear_ratio,
            description: format!(
                "Smart Money Distribution | {:.1}% of last-10 volume is selling | institutions unloading — bearish continuation probable",
                bear_ratio * 100.0
            ),
            detected_at: Utc::now(),
        });
    }

    out
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Group price levels into clusters within `tolerance` of each other.
/// Returns (representative_level, cluster_size) pairs.
fn cluster_levels(mut levels: Vec<f64>, tolerance: f64) -> Vec<(f64, usize)> {
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clusters: Vec<(f64, usize)> = Vec::new();

    for price in levels {
        if let Some(last) = clusters.last_mut() {
            if (price - last.0).abs() / last.0 <= tolerance {
                // Merge into existing cluster (update mean)
                last.0 = (last.0 * last.1 as f64 + price) / (last.1 + 1) as f64;
                last.1 += 1;
                continue;
            }
        }
        clusters.push((price, 1));
    }

    clusters
}
