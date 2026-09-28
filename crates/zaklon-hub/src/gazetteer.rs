//! "Find a place" for setting the household's home: the towns and cities of
//! the world (GeoNames, about 70,000 places of 5,000 people or more) from the
//! map assets' `cities.tsv`, searched by name on the hub. See
//! scripts/map-cities.mjs for the file.

use std::path::Path;

use serde::Serialize;

/// The list of places in the map assets.
pub const PLACES_FILE: &str = "cities.tsv";
/// The most places one search returns.
pub const MAX_RESULTS: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct Place {
    pub name: String,
    /// The Serbian (Latin) name, where it differs: Beograd, Beč.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_sr: Option<String>,
    /// The region (state, province) it is in, in English.
    pub region: String,
    /// The country's two-letter code.
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    pub population: u64,
    /// The folded names it is found by.
    #[serde(skip)]
    keys: Vec<String>,
}

pub struct Gazetteer {
    /// The largest first.
    places: Vec<Place>,
}

impl Gazetteer {
    /// Read the list (blocking; a few megabytes).
    pub fn load(path: &Path) -> std::io::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path)?))
    }

    pub fn parse(text: &str) -> Self {
        let places = text
            .lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.split('\t').collect();
                let [name, ascii, sr, region, country, lat, lon, population] = f.as_slice() else { return None };
                let (lat, lon) = (lat.parse::<f64>().ok()?, lon.parse::<f64>().ok()?);
                if name.is_empty() || !lat.is_finite() || !lon.is_finite() {
                    return None;
                }
                let mut keys: Vec<String> = [*name, *ascii, *sr].iter().filter(|n| !n.is_empty()).map(|n| fold(n)).collect();
                keys.dedup();
                Some(Place {
                    name: name.to_string(),
                    name_sr: (!sr.is_empty()).then(|| sr.to_string()),
                    region: region.to_string(),
                    country: country.to_string(),
                    lat,
                    lon,
                    population: population.parse().unwrap_or(0),
                    keys,
                })
            })
            .collect();
        Self { places }
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The places whose name (in any of its forms) is `query` or starts with
    /// it, then those with a word starting with it; the larger first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Place> {
        let q = fold(query);
        if q.chars().count() < 2 {
            return Vec::new();
        }
        let rank = |p: &Place| {
            p.keys
                .iter()
                .filter_map(|k| {
                    if *k == q {
                        Some(0)
                    } else if k.starts_with(&q) {
                        Some(1)
                    } else if k.contains(&format!(" {q}")) {
                        Some(2)
                    } else {
                        None
                    }
                })
                .min()
        };
        let mut found: Vec<(u8, usize)> = self.places.iter().enumerate().filter_map(|(i, p)| rank(p).map(|r| (r, i))).collect();
        // By how well it matches, then by size (the list is largest first).
        found.sort_unstable();
        found.into_iter().take(limit).map(|(_, i)| self.places[i].clone()).collect()
    }
}

/// Lower case, Latin letters without their marks, punctuation as spaces: a
/// form in which "Niš", "nis" and "NIS" compare equal. Other scripts stay.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
            'ď' | 'đ' | 'ð' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
            'ĥ' | 'ħ' => "h",
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
            'ĵ' => "j",
            'ķ' => "k",
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
            'ñ' | 'ń' | 'ņ' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
            'ŕ' | 'ŗ' | 'ř' => "r",
            'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
            'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
            'ŵ' => "w",
            'ý' | 'ÿ' | 'ŷ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            'þ' => "th",
            '\u{0300}'..='\u{036F}' => "",
            c if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(plain);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "Belgrade\tBelgrade\tBeograd\tCentral Serbia\tRS\t44.8040\t20.4651\t1273651\n\
                        Vienna\tVienna\tBeč\tVienna\tAT\t48.2085\t16.3721\t1691468\n\
                        Niš\tNis\t\tCentral Serbia\tRS\t43.3247\t21.9033\t250000\n\
                        Novi Sad\tNovi Sad\t\tVojvodina\tRS\t45.2517\t19.8369\t215400\n\
                        Belgrade\tBelgrade\t\tMontana\tUS\t45.7760\t-111.1769\t8029\n\
                        Stara Pazova\tStara Pazova\t\tVojvodina\tRS\t44.9850\t20.1608\t18602\n\
                        broken line\n";

    #[test]
    fn finds_places_by_any_of_their_names_the_larger_first() {
        let g = Gazetteer::parse(LIST);
        assert_eq!(g.len(), 6, "the broken line is left out");
        let names = |q: &str| g.search(q, MAX_RESULTS).into_iter().map(|p| format!("{} {}", p.name, p.country)).collect::<Vec<_>>();
        assert_eq!(names("beograd"), ["Belgrade RS"], "by its Serbian name");
        assert_eq!(names("Belgrade"), ["Belgrade RS", "Belgrade US"], "the larger first");
        assert_eq!(names("bec"), ["Vienna AT"], "without the marks");
        assert_eq!(names("Beč"), ["Vienna AT"]);
        assert_eq!(names("nis"), ["Niš RS"]);
        assert_eq!(names("NIŠ"), ["Niš RS"]);
        assert_eq!(names("sad"), ["Novi Sad RS"], "a word of the name");
        assert_eq!(names("pazova"), ["Stara Pazova RS"]);
        assert_eq!(names("novi s"), ["Novi Sad RS"]);
        assert!(names("b").is_empty(), "too short to search");
        assert!(names("zzz").is_empty());
        let p = &g.search("beograd", 1)[0];
        assert_eq!((p.name_sr.as_deref(), p.lat, p.lon, p.population), (Some("Beograd"), 44.804, 20.4651, 1_273_651));
        assert_eq!(g.search("belgrade", 1).len(), 1, "no more than asked for");
    }

    #[test]
    fn folding_ignores_case_marks_and_punctuation() {
        assert_eq!(fold("Niš"), "nis");
        assert_eq!(fold("  Đakovo "), "dakovo");
        assert_eq!(fold("Saint-Étienne"), "saint etienne");
        assert_eq!(fold("München"), "munchen");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Београд"), "београд", "other scripts stay");
    }
}
