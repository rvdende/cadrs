//! Materials (PS10, X9, P3.5): what a part is made of, for its mass.
//!
//! A part's material is stored with the part ([`crate::PartProps::material`]), a copy of a
//! [`LIBRARY`] entry or a custom one (the Assign material dialog's Custom tab, PS10.3). Values
//! are SI: density in kg/m³, moduli and strengths in Pa.
//!
//! The library's names follow the common trade names; its values are typical published values,
//! each entry citing where they come from:
//! - **ASM**: ASM Aerospace Specification Metals / ASM Handbook data sheets as reproduced on
//!   MatWeb (asm.matweb.com) for the named alloy and temper.
//! - **EN 1993-1-1**: Eurocode 3, §3.2.6 (structural steel: ρ 7850 kg/m³, E 210 GPa, ν 0.3) and
//!   Table 3.1 (S235: fy 235 MPa, fu 360 MPa).
//! - **MatWeb**: MatWeb's "Overview of materials for …" averages for the named polymer.
//!
//! Every entry has Young's modulus and Poisson's ratio, which the simulation (P3F.5,
//! [`crate::simulation`]) reads; the stiffness of the course's simulation beam (E = 200 GPa) is
//! the **Steel - A36** entry's.
//! - The Polypropylene entry reproduces the property list of the course's screenshot
//!   (`training/intro-to-part-studios/ex4-step17.png`: 0.033 lb/in³, ν 0.43, E 213205.474 psi,
//!   yield 4728.23 psi, ultimate 10819.815 psi, compressive yield 1450.377 psi): 1.47 GPa,
//!   32.6 MPa, 74.6 MPa and 10 MPa, and a density of exactly **0.033 lb/in³ = 913.437 kg/m³**
//!   (P3.7). The density is the course's own: the funnel's Mass and section properties
//!   (`ex4-step18.png`: 0.098 lb, Lxx 0.205, Lyy 0.518, Lxz −0.059 in² lb for V 2.974 in³) all
//!   round as shown only for ρ in 911.9–913.6 kg/m³ (MatWeb's 0.900 g/cm³ homopolymer gives
//!   0.097 lb and Lyy 0.511), so Onshape's library stores 0.033 lb/in³ itself.

use serde::{Deserialize, Serialize};

/// A named library of custom materials, saved in the document (PS10.3, P3.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialLibrary {
    pub name: String,
    pub materials: Vec<Material>,
}

impl MaterialLibrary {
    /// The library's material called `name`, marked as from this library.
    pub fn material(&self, name: &str) -> Option<Material> {
        self.materials.iter().find(|m| m.name == name).map(|m| Material { library: Some(self.name.clone()), ..m.clone() })
    }
}

/// The next free library name, "Library 1", "Library 2", ….
pub fn next_library_name(libraries: &[MaterialLibrary]) -> String {
    (1..).map(|n| format!("Library {n}")).find(|n| !libraries.iter().any(|l| l.name == *n) && n != LIBRARY_NAME).unwrap_or_default()
}

/// A part's material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// kg/m³.
    pub density: f64,
    #[serde(default)]
    pub poisson: Option<f64>,
    /// Young's modulus, Pa.
    #[serde(default)]
    pub youngs_modulus: Option<f64>,
    /// Tensile yield strength, Pa.
    #[serde(default)]
    pub tensile_yield: Option<f64>,
    /// Ultimate tensile strength, Pa.
    #[serde(default)]
    pub ultimate_tensile: Option<f64>,
    /// Compressive yield strength, Pa.
    #[serde(default)]
    pub compressive_yield: Option<f64>,
    /// Ultimate compressive strength, Pa.
    #[serde(default)]
    pub ultimate_compressive: Option<f64>,
    /// The library it came from ([`LIBRARY_NAME`]), `None` for a custom material.
    #[serde(default)]
    pub library: Option<String>,
}

impl Material {
    /// Density in kg/mm³ (mass properties work in mm).
    pub fn density_kg_mm3(&self) -> f64 {
        self.density * 1e-9
    }

    /// A custom material (the Custom tab): a name and a density.
    pub fn custom(name: impl Into<String>, density: f64) -> Self {
        Self {
            name: name.into(),
            density,
            poisson: None,
            youngs_modulus: None,
            tensile_yield: None,
            ultimate_tensile: None,
            compressive_yield: None,
            ultimate_compressive: None,
            library: None,
        }
    }
}

/// The bundled library's name in the dialog's library dropdown.
pub const LIBRARY_NAME: &str = "Standard materials";

/// A library entry (constant data; [`LibraryMaterial::material`] makes a [`Material`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LibraryMaterial {
    pub name: &'static str,
    /// kg/m³.
    pub density: f64,
    pub poisson: f64,
    /// GPa.
    pub youngs_gpa: f64,
    /// MPa (0: not given).
    pub tensile_yield_mpa: f64,
    pub ultimate_tensile_mpa: f64,
    pub compressive_yield_mpa: f64,
    pub ultimate_compressive_mpa: f64,
    /// Where the values come from.
    pub source: &'static str,
}

impl LibraryMaterial {
    pub fn material(&self) -> Material {
        let mpa = |v: f64| (v > 0.0).then_some(v * 1e6);
        Material {
            name: self.name.to_string(),
            density: self.density,
            poisson: Some(self.poisson),
            youngs_modulus: Some(self.youngs_gpa * 1e9),
            tensile_yield: mpa(self.tensile_yield_mpa),
            ultimate_tensile: mpa(self.ultimate_tensile_mpa),
            compressive_yield: mpa(self.compressive_yield_mpa),
            ultimate_compressive: Some(self.ultimate_compressive_mpa * 1e6),
            library: Some(LIBRARY_NAME.to_string()),
        }
    }
}

const fn m(
    name: &'static str,
    density: f64,
    poisson: f64,
    youngs_gpa: f64,
    [tensile_yield_mpa, ultimate_tensile_mpa, compressive_yield_mpa, ultimate_compressive_mpa]: [f64; 4],
    source: &'static str,
) -> LibraryMaterial {
    LibraryMaterial {
        name,
        density,
        poisson,
        youngs_gpa,
        tensile_yield_mpa,
        ultimate_tensile_mpa,
        compressive_yield_mpa,
        ultimate_compressive_mpa,
        source,
    }
}

/// Polypropylene's density, kg/m³: 0.033 lb/in³ (0.033 × 0.453 592 37 / 0.0254³), see the module
/// docs.
pub const POLYPROPYLENE_DENSITY: f64 = 0.033 * 0.453_592_37 / (0.0254 * 0.0254 * 0.0254);

/// The bundled material library, alphabetical.
pub const LIBRARY: &[LibraryMaterial] = &[
    m("ABS", 1040.0, 0.35, 2.3, [43.0, 44.0, 0.0, 0.0], "MatWeb: ABS, molded (averages)"),
    m("Acetal", 1410.0, 0.35, 2.9, [69.0, 69.0, 0.0, 0.0], "MatWeb: acetal copolymer (POM), unfilled"),
    m("Acrylic", 1190.0, 0.37, 3.2, [0.0, 72.0, 0.0, 124.0], "MatWeb: PMMA (acrylic), cast"),
    m("Aluminum - 1060", 2705.0, 0.33, 69.0, [27.6, 69.0, 0.0, 0.0], "ASM: aluminum 1060-O"),
    m("Aluminum - 380", 2760.0, 0.33, 71.0, [159.0, 324.0, 0.0, 0.0], "ASM: aluminum 380.0-F die casting"),
    m("Aluminum - 6061", 2700.0, 0.33, 68.9, [276.0, 310.0, 0.0, 0.0], "ASM: aluminum 6061-T6"),
    m("Aluminum - 7075", 2810.0, 0.33, 71.7, [503.0, 572.0, 0.0, 0.0], "ASM: aluminum 7075-T6"),
    m("Brass", 8500.0, 0.31, 97.0, [124.0, 338.0, 0.0, 0.0], "MatWeb: free-cutting brass C36000, annealed"),
    m("Bronze", 8830.0, 0.34, 100.0, [125.0, 240.0, 0.0, 0.0], "MatWeb: bearing bronze C93200 (SAE 660), as cast"),
    // P3I.8: the sheet metal exercises' (E1, E2) "Carbon steel".
    m("Carbon Steel", 7850.0, 0.29, 205.0, [350.0, 420.0, 0.0, 0.0], "MatWeb: AISI 1020 steel, cold drawn"),
    m("Cast Iron", 7150.0, 0.26, 110.0, [0.0, 293.0, 0.0, 965.0], "MatWeb: gray cast iron ASTM A48 class 40"),
    m("Copper", 8890.0, 0.31, 115.0, [69.0, 220.0, 0.0, 0.0], "MatWeb: copper C11000, annealed"),
    m("Nylon 6/6", 1140.0, 0.41, 2.9, [0.0, 82.7, 0.0, 0.0], "MatWeb: nylon 66, unfilled, dry as molded"),
    m("PLA", 1240.0, 0.36, 3.5, [60.0, 65.0, 0.0, 0.0], "MatWeb: polylactic acid (PLA), injection molded"),
    m("Polycarbonate", 1200.0, 0.37, 2.38, [62.0, 65.0, 0.0, 0.0], "MatWeb: polycarbonate, unfilled"),
    m(
        "Polypropylene",
        POLYPROPYLENE_DENSITY,
        0.43,
        1.47,
        [32.6, 74.6, 10.0, 0.0],
        "Onshape's library as the course's ex4-step17.png lists it (0.033 lb/in³; moduli and strengths in round MPa), confirmed by ex4-step18.png's funnel mass and inertia",
    ),
    m("Stainless Steel - 304", 8000.0, 0.29, 193.0, [215.0, 505.0, 0.0, 0.0], "ASM: AISI type 304, annealed"),
    m("Steel", 7850.0, 0.3, 210.0, [235.0, 360.0, 0.0, 0.0], "EN 1993-1-1 §3.2.6 and Table 3.1 (S235)"),
    // P3F.5: the course's simulation beam is steel with E = 200 GPa.
    m("Steel - A36", 7850.0, 0.26, 200.0, [250.0, 400.0, 0.0, 0.0], "ASM: ASTM A36 steel, bar (E 200 GPa, ν 0.26, yield 250 MPa, ultimate 400–550 MPa)"),
    m("Titanium - Ti-6Al-4V", 4430.0, 0.342, 113.8, [880.0, 950.0, 970.0, 0.0], "ASM: Ti-6Al-4V (grade 5), annealed"),
];

/// A library material by name.
pub fn library(name: &str) -> Option<Material> {
    LIBRARY.iter().find(|m| m.name == name).map(LibraryMaterial::material)
}

/// The library's materials whose names contain `query` (any case), in order (the dialog's
/// searchable dropdown).
pub fn search(query: &str) -> Vec<&'static LibraryMaterial> {
    let q = query.trim().to_lowercase();
    LIBRARY
        .iter()
        .filter(|m| q.is_empty() || m.name.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_has_the_course_materials() {
        for name in ["Polypropylene", "Aluminum - 380", "Aluminum - 1060", "Steel", "ABS", "Brass", "Aluminum - 6061"] {
            assert!(library(name).is_some(), "{name}");
        }
        assert!(LIBRARY.windows(2).all(|w| w[0].name < w[1].name), "alphabetical");
        assert!(LIBRARY.iter().all(|m| m.density > 0.0 && !m.source.is_empty()));
    }

    #[test]
    fn polypropylene_reads_as_in_the_course() {
        // ex4-step17.png: Density (lb/in³) 0.033, Young's modulus (psi) 213205.474, Tensile yield
        // 4728.23, Ultimate tensile 10819.815, Compressive yield 1450.377. 1 lb/in³ =
        // 0.45359237 / 0.0254³ = 27679.905 kg/m³; 1 psi = 6894.757 Pa.
        let pp = library("Polypropylene").unwrap();
        let lb_in3 = pp.density / 27_679.904_7;
        assert!((lb_in3 - 0.033).abs() < 1e-9, "{lb_in3}");
        assert!((pp.density - 913.437).abs() < 1e-3);
        let psi = 6_894.757_293;
        assert!((pp.youngs_modulus.unwrap() / psi - 213_205.474).abs() < 1.0);
        assert!((pp.tensile_yield.unwrap() / psi - 4_728.23).abs() < 1.0);
        assert!((pp.ultimate_tensile.unwrap() / psi - 10_819.815).abs() < 1.0);
        assert!((pp.compressive_yield.unwrap() / psi - 1_450.377).abs() < 0.1);
    }

    #[test]
    fn search_is_case_insensitive() {
        let hits: Vec<&str> = search("alum").iter().map(|m| m.name).collect();
        assert_eq!(hits, ["Aluminum - 1060", "Aluminum - 380", "Aluminum - 6061", "Aluminum - 7075"]);
        assert_eq!(search("").len(), LIBRARY.len());
    }
}
