//! Instant quotes: a price from the part's measurements and the shop's rate card.
//!
//! Profile cutting is priced the way shops do it by hand: machine time (cutting at the
//! material's speed for its thickness, plus a pierce per cut) at the machine's hourly rate,
//! material by the sheet area the parts take (with a nesting allowance), and a setup fee per
//! order. The shop sees the working and can change the price before sending it.

use serde::{Deserialize, Serialize};

use crate::capability::{MachineCaps, PartSummary, fit};

/// Money in the smallest unit of a currency (cents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Money(pub i64);

impl std::ops::Add for Money {
    type Output = Money;
    fn add(self, other: Money) -> Money {
        Money(self.0 + other.0)
    }
}

impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        Money(iter.map(|m| m.0).sum())
    }
}

impl Money {
    pub fn from_major(major: f64) -> Self {
        Money((major * 100.0).round() as i64)
    }

    /// "1 234.50".
    pub fn format(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let cents = self.0.unsigned_abs();
        let whole = (cents / 100).to_string();
        let mut grouped = String::new();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                grouped.push(' ');
            }
            grouped.push(c);
        }
        format!("{sign}{grouped}.{:02}", cents % 100)
    }
}

/// What a shop charges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RateCard {
    /// ISO 4217 ("ZAR", "EUR").
    pub currency: String,
    /// Per order.
    pub setup: Money,
    /// The least an order costs.
    pub minimum: Money,
    /// Machine time per hour, by machine name.
    pub hourly: Vec<(String, Money)>,
    /// Material per square metre of sheet, by (material, thickness mm).
    pub sheet_per_m2: Vec<(String, f64, Money)>,
    /// How much more sheet than the parts' area nesting uses (1.25 = a quarter more).
    pub nesting_allowance: f64,
}

impl RateCard {
    fn hourly(&self, machine: &str) -> Option<Money> {
        self.hourly
            .iter()
            .find(|(name, _)| name == machine)
            .map(|(_, rate)| *rate)
    }

    fn sheet(&self, material: &str, thickness: f64) -> Option<Money> {
        self.sheet_per_m2
            .iter()
            .filter(|(m, t, _)| m.eq_ignore_ascii_case(material) && *t >= thickness - 1e-6)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(_, _, price)| *price)
    }
}

/// One part's line on a quote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub part: String,
    pub quantity: u32,
    pub machine: String,
    /// Machine minutes for all of them.
    pub minutes: f64,
    pub machine_cost: Money,
    pub material_cost: Money,
    pub total: Money,
}

/// A worked-out price.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub lines: Vec<Line>,
    pub setup: Money,
    pub total: Money,
    pub currency: String,
}

/// Why no price could be worked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoPrice {
    /// No machine can make this part; why each can't.
    NoMachine { part: String, why: Vec<String> },
    /// The rate card has no rate for what's needed.
    NoRate(String),
}

/// Prices `parts` (with quantities) on the cheapest machine that can make each.
pub fn estimate(
    machines: &[MachineCaps],
    rates: &RateCard,
    parts: &[(PartSummary, u32)],
) -> Result<Estimate, NoPrice> {
    let mut lines = Vec::new();
    for (part, quantity) in parts {
        let mut best: Option<Line> = None;
        let mut why = Vec::new();
        for machine in machines {
            let fit = fit(machine, part);
            if !fit.ok() {
                why.extend(
                    fit.problems
                        .iter()
                        .map(|p| format!("{}: {p}", machine.name)),
                );
                continue;
            }
            match line(machine, rates, part, *quantity) {
                Ok(line) => {
                    if best.as_ref().is_none_or(|b| line.total < b.total) {
                        best = Some(line);
                    }
                }
                Err(NoPrice::NoRate(r)) => why.push(format!("{}: no rate for {r}", machine.name)),
                Err(e) => return Err(e),
            }
        }
        lines.push(best.ok_or_else(|| NoPrice::NoMachine {
            part: part.name.clone(),
            why,
        })?);
    }
    let parts_total: Money = lines.iter().map(|l| l.total).sum();
    let total = (parts_total + rates.setup).max(rates.minimum);
    Ok(Estimate {
        lines,
        setup: rates.setup,
        total,
        currency: rates.currency.clone(),
    })
}

fn line(
    machine: &MachineCaps,
    rates: &RateCard,
    part: &PartSummary,
    quantity: u32,
) -> Result<Line, NoPrice> {
    let hourly = rates
        .hourly(&machine.name)
        .ok_or_else(|| NoPrice::NoRate("machine time".into()))?;
    let (minutes, material_cost) = if machine.process.cuts_profiles() {
        let thickness = part.flat_thickness.unwrap_or_default();
        let material = part.material.clone().unwrap_or_default();
        let speed = machine
            .materials
            .iter()
            .find(|m| m.material.eq_ignore_ascii_case(&material))
            .and_then(|m| m.speed_at(thickness))
            .or_else(|| machine.process.typical_speed(thickness))
            .filter(|s| *s > 0.0)
            .ok_or_else(|| NoPrice::NoRate(format!("cutting speed in {material}")))?;
        let minutes = part.cut_length / speed + part.pierces as f64 * machine.pierce_seconds / 60.0;
        let per_m2 = rates
            .sheet(&material, thickness)
            .ok_or_else(|| NoPrice::NoRate(format!("{thickness} mm {material} sheet")))?;
        let m2 = part.area / 1e6 * rates.nesting_allowance;
        (minutes, Money((per_m2.0 as f64 * m2).round() as i64))
    } else {
        return Err(NoPrice::NoRate(format!(
            "{} work (quoted by hand)",
            machine.process.label()
        )));
    };
    let q = quantity as f64;
    let machine_cost = Money((hourly.0 as f64 * minutes * q / 60.0).round() as i64);
    let material_cost = Money(material_cost.0 * quantity as i64);
    Ok(Line {
        part: part.name.clone(),
        quantity,
        machine: machine.name.clone(),
        minutes: minutes * q,
        machine_cost,
        material_cost,
        total: machine_cost + material_cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::tests::{bracket, waterjet};

    fn rates() -> RateCard {
        RateCard {
            currency: "ZAR".into(),
            setup: Money::from_major(250.0),
            minimum: Money::from_major(500.0),
            hourly: vec![("Flow Mach 500".into(), Money::from_major(1200.0))],
            sheet_per_m2: vec![
                ("mild steel".into(), 6.0, Money::from_major(600.0)),
                ("mild steel".into(), 10.0, Money::from_major(1000.0)),
            ],
            nesting_allowance: 1.25,
        }
    }

    #[test]
    fn six_steel_brackets() {
        let estimate = estimate(&[waterjet()], &rates(), &[(bracket(), 6)]).unwrap();
        let line = &estimate.lines[0];
        // 900 mm at 300 mm/min = 3 min, plus 3 pierces of 4 s = 0.2 min: 3.2 min each.
        assert!((line.minutes - 19.2).abs() < 1e-9);
        assert_eq!(line.machine_cost, Money::from_major(384.0));
        // 0.012 m² × 1.25 × R1000 = R15 each.
        assert_eq!(line.material_cost, Money::from_major(90.0));
        assert_eq!(estimate.total, Money::from_major(724.0));
    }

    #[test]
    fn the_minimum_applies() {
        let estimate = estimate(&[waterjet()], &rates(), &[(bracket(), 1)]).unwrap();
        assert_eq!(estimate.total, Money::from_major(500.0));
    }

    #[test]
    fn says_why_there_is_no_price() {
        let mut part = bracket();
        part.material = Some("titanium".into());
        match estimate(&[waterjet()], &rates(), &[(part, 1)]) {
            Err(NoPrice::NoMachine { why, .. }) => assert!(why[0].contains("titanium")),
            other => panic!("{other:?}"),
        }
        let mut thick = bracket();
        thick.flat_thickness = Some(20.0);
        assert!(matches!(
            estimate(&[waterjet()], &rates(), &[(thick, 1)]),
            Err(NoPrice::NoMachine { .. })
        ));
    }

    #[test]
    fn money_formats() {
        assert_eq!(Money::from_major(1234.5).format(), "1 234.50");
        assert_eq!(Money(-5).format(), "-0.05");
        assert_eq!(Money::from_major(1_000_000.0).format(), "1 000 000.00");
    }
}
