#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Place {
    pub name: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
}

pub fn nearest(lat: f64, lon: f64) -> Place {
    static GEOCODER: std::sync::OnceLock<reverse_geocoder::ReverseGeocoder> =
        std::sync::OnceLock::new();
    let r = GEOCODER
        .get_or_init(reverse_geocoder::ReverseGeocoder::new)
        .search((lat, lon))
        .record;
    let some = |s: &str| Some(s.to_owned()).filter(|s| !s.is_empty());
    Place {
        name: None,
        city: some(&r.name),
        state: some(&r.admin1),
        country: some(&r.cc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_city() {
        let p = nearest(47.6785, -122.1305);
        assert_eq!(p.country.as_deref(), Some("US"));
        assert_eq!(p.state.as_deref(), Some("Washington"));
        assert!(p.city.is_some());
        assert_eq!(p.name, None);
    }
}
