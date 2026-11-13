//! Where someone is, as precisely as they choose to say.
//!
//! A place names its body first, so a shop on the Moon or a machine in orbit fits later without a
//! new format. Today the app sets Earth surface places; the other frames are carried but not
//! drawn. A place can say how precise it is ([`Place::accuracy_m`]) and give a region by name, so a
//! shop can show "Stellenbosch" publicly while its contacts see the street. [`Place::coarsened`]
//! is what goes into public listings.

use serde::{Deserialize, Serialize};

/// The body a place is on or around.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Body {
    Earth,
    Moon,
    Mars,
    Other(String),
}

impl Body {
    /// Mean radius in metres, when known.
    pub fn radius_m(&self) -> Option<f64> {
        match self {
            Body::Earth => Some(6_371_008.8),
            Body::Moon => Some(1_737_400.0),
            Body::Mars => Some(3_389_500.0),
            Body::Other(_) => None,
        }
    }
}

/// Where on or around the body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Frame {
    /// On the surface: latitude and longitude in degrees, height above the mean radius in metres.
    Surface { lat: f64, lon: f64, alt_m: f64 },
    /// In orbit: Keplerian elements at an epoch (milliseconds since the Unix epoch).
    Orbit {
        semi_major_axis_m: f64,
        eccentricity: f64,
        inclination_deg: f64,
        ascending_node_deg: f64,
        periapsis_deg: f64,
        mean_anomaly_deg: f64,
        epoch: u64,
    },
    /// Somewhere the frames above don't cover, as free text for now.
    Elsewhere(String),
}

/// A named region, broadest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub country: Option<String>,
    pub area: Option<String>,
    pub city: Option<String>,
}

impl Region {
    /// "Stellenbosch, Western Cape, South Africa".
    pub fn label(&self) -> String {
        [&self.city, &self.area, &self.country]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Where someone is. Every part is optional but the body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub body: Body,
    pub frame: Option<Frame>,
    /// How far off the frame may be, in metres: how precise the owner chose to be.
    pub accuracy_m: Option<f64>,
    pub region: Option<Region>,
}

impl Place {
    /// A point on Earth's surface.
    pub fn earth(lat: f64, lon: f64) -> Self {
        Self {
            body: Body::Earth,
            frame: Some(Frame::Surface {
                lat,
                lon,
                alt_m: 0.0,
            }),
            accuracy_m: None,
            region: None,
        }
    }

    /// The place as precise as `accuracy_m` and no more: a surface point snapped to a grid of
    /// about that size, so a listing can't give away the street.
    pub fn coarsened(&self, accuracy_m: f64) -> Self {
        let mut place = self.clone();
        let accuracy_m = accuracy_m.max(self.accuracy_m.unwrap_or(0.0));
        if let (Some(Frame::Surface { lat, lon, .. }), Some(radius)) =
            (&self.frame, self.body.radius_m())
        {
            let step_deg = (accuracy_m / radius).to_degrees().max(1e-7);
            let snap = |v: f64| (v / step_deg).round() * step_deg;
            place.frame = Some(Frame::Surface {
                lat: snap(*lat).clamp(-90.0, 90.0),
                lon: snap(*lon),
                alt_m: 0.0,
            });
        }
        place.accuracy_m = Some(accuracy_m);
        place
    }

    /// Distance along the surface to `other` in metres, when both are surface points on the same
    /// body of known size.
    pub fn distance_m(&self, other: &Place) -> Option<f64> {
        if self.body != other.body {
            return None;
        }
        let radius = self.body.radius_m()?;
        let (
            Some(Frame::Surface { lat: a, lon: b, .. }),
            Some(Frame::Surface { lat: c, lon: d, .. }),
        ) = (&self.frame, &other.frame)
        else {
            return None;
        };
        let (a, b, c, d) = (
            a.to_radians(),
            b.to_radians(),
            c.to_radians(),
            d.to_radians(),
        );
        let h = ((c - a) / 2.0).sin().powi(2) + a.cos() * c.cos() * ((d - b) / 2.0).sin().powi(2);
        Some(2.0 * radius * h.sqrt().clamp(0.0, 1.0).asin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_between_cities() {
        let cape_town = Place::earth(-33.9249, 18.4241);
        let johannesburg = Place::earth(-26.2041, 28.0473);
        let km = cape_town.distance_m(&johannesburg).unwrap() / 1000.0;
        assert!((km - 1_263.0).abs() < 10.0, "{km}");
        assert_eq!(cape_town.distance_m(&cape_town), Some(0.0));
    }

    #[test]
    fn no_distance_between_bodies() {
        let mut moon = Place::earth(0.0, 0.0);
        moon.body = Body::Moon;
        assert_eq!(Place::earth(0.0, 0.0).distance_m(&moon), None);
    }

    #[test]
    fn coarsening_moves_the_point_no_further_than_its_accuracy() {
        let shop = Place::earth(-33.93571, 18.86112);
        let public = shop.coarsened(5_000.0);
        assert_eq!(public.accuracy_m, Some(5_000.0));
        assert_ne!(public.frame, shop.frame);
        assert!(shop.distance_m(&public).unwrap() < 5_000.0);
        // Two shops in one grid cell coarsen to the same point.
        let neighbour = Place::earth(-33.93500, 18.86000);
        assert_eq!(neighbour.coarsened(5_000.0).frame, public.frame);
    }

    #[test]
    fn region_labels() {
        let region = Region {
            country: Some("South Africa".into()),
            area: None,
            city: Some("Stellenbosch".into()),
        };
        assert_eq!(region.label(), "Stellenbosch, South Africa");
    }
}
