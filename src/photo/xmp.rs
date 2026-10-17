use super::Asset;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn coord(value: f64, pos: char, neg: char) -> String {
    let abs = value.abs();
    let deg = abs.trunc();
    format!(
        "{deg},{:.6}{}",
        (abs - deg) * 60.0,
        if value < 0.0 { neg } else { pos }
    )
}

fn bag(tag: &str, items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let li: String = items
        .iter()
        .map(|i| format!("     <rdf:li>{}</rdf:li>\n", esc(i)))
        .collect();
    format!("   <{tag}>\n    <rdf:Bag>\n{li}    </rdf:Bag>\n   </{tag}>\n")
}

fn text(tag: &str, value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() => format!("   <{tag}>{}</{tag}>\n", esc(v)),
        _ => String::new(),
    }
}

fn alt(tag: &str, value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() => format!(
            "   <{tag}>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n    </rdf:Alt>\n   </{tag}>\n",
            esc(v)
        ),
        _ => String::new(),
    }
}

pub fn write(asset: &Asset) -> String {
    let date = asset.taken.map(|t| t.to_rfc3339());
    let mut body = String::new();
    body += &alt("dc:title", asset.title.as_deref());
    body += &alt("dc:description", asset.description.as_deref());
    body += &bag("dc:subject", &asset.keywords);
    body += &bag("Iptc4xmpExt:PersonInImage", &asset.people);
    body += &text("xmp:CreateDate", date.as_deref());
    body += &text("photoshop:DateCreated", date.as_deref());
    body += &text("exif:DateTimeOriginal", date.as_deref());
    if asset.favorite {
        body += "   <xmp:Rating>5</xmp:Rating>\n";
    }
    if let (Some(lat), Some(lon)) = (asset.latitude, asset.longitude) {
        body += &text("exif:GPSLatitude", Some(&coord(lat, 'N', 'S')));
        body += &text("exif:GPSLongitude", Some(&coord(lon, 'E', 'W')));
    }
    body += &text("Iptc4xmpExt:Sublocation", asset.place.name.as_deref());
    body += &text("Iptc4xmpExt:City", asset.place.city.as_deref());
    body += &text("Iptc4xmpExt:ProvinceState", asset.place.state.as_deref());
    body += &text("Iptc4xmpExt:CountryName", asset.place.country.as_deref());
    body += &text("tiff:Make", asset.camera.make.as_deref());
    body += &text("tiff:Model", asset.camera.model.as_deref());
    body += &text("exifEX:LensModel", asset.camera.lens.as_deref());
    body += &bag("photosink:albums", &asset.albums);
    body += &bag("photosink:labels", &asset.labels);
    body += &text(
        "photosink:asset",
        Some(&serde_json::to_string(asset).unwrap_or_default()),
    );
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
<rdf:Description rdf:about=\"\"\n    \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n    \
xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n    \
xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"\n    \
xmlns:exif=\"http://ns.adobe.com/exif/1.0/\"\n    \
xmlns:exifEX=\"http://cipa.jp/exif/1.0/\"\n    \
xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n    \
xmlns:Iptc4xmpExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\"\n    \
xmlns:photosink=\"https://github.com/n8mgr/photosink/ns/1.0/\">\n\
{body}  \
</rdf:Description>\n \
</rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>\n"
    )
}

pub fn read(xmp: &str) -> Option<Asset> {
    let start = xmp.find("<photosink:asset>")? + "<photosink:asset>".len();
    let end = xmp[start..].find("</photosink:asset>")? + start;
    serde_json::from_str(&unesc(&xmp[start..end])).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::Camera;
    use crate::photo::geo::Place;

    #[test]
    fn round_trips_and_formats() {
        let asset = Asset {
            uuid: "u".into(),
            filename: "IMG_1.CR3".into(),
            title: Some("A <b> & \"c\"".into()),
            description: Some("desc".into()),
            taken: crate::sources::apple_photos::library::cocoa_time(0.0, 3600),
            latitude: Some(48.8566),
            longitude: Some(-2.3522),
            favorite: true,
            keywords: vec!["beach".into()],
            people: vec!["Ann".into()],
            albums: vec!["Trip".into()],
            camera: Camera {
                make: Some("Canon".into()),
                model: Some("EOS R5".into()),
                lens: None,
            },
            place: Place {
                city: Some("Paris".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let xmp = write(&asset);
        assert!(
            xmp.contains("<rdf:li xml:lang=\"x-default\">A &lt;b&gt; &amp; &quot;c&quot;</rdf:li>")
        );
        assert!(xmp.contains("<exif:GPSLatitude>48,51.396000N</exif:GPSLatitude>"));
        assert!(xmp.contains("<exif:GPSLongitude>2,21.132000W</exif:GPSLongitude>"));
        assert!(xmp.contains("<xmp:CreateDate>2001-01-01T01:00:00+01:00</xmp:CreateDate>"));
        assert!(xmp.contains("<xmp:Rating>5</xmp:Rating>"));
        assert!(xmp.contains("<Iptc4xmpExt:City>Paris</Iptc4xmpExt:City>"));
        assert!(xmp.contains("<Iptc4xmpExt:PersonInImage>"));
        assert!(!xmp.contains("exifEX:LensModel"));
        let back = read(&xmp).unwrap();
        assert_eq!(back.title, asset.title);
        assert_eq!(back.taken, asset.taken);
        assert_eq!(back.keywords, asset.keywords);
        assert_eq!(back.camera, asset.camera);
        assert_eq!(read("<x/>"), None);
    }
}
