//! The bundled Inter faces resolve under the family name the tokens request,
//! with the same matcher iced's text stack uses (`fontdb`).

#![cfg(feature = "bundled-font")]

use fontdb::{Database, Family, Query, Style};
use iced_luminate::theme::typography::{
    DECLARED_WEIGHTS, FAMILY, FONT, FONT_INTER, FONT_INTER_ITALIC,
};

fn database() -> Database {
    let mut db = Database::new();
    db.load_font_data(FONT_INTER.to_vec());
    db.load_font_data(FONT_INTER_ITALIC.to_vec());
    db
}

#[test]
fn the_family_name_resolves_to_the_upright_face() {
    let db = database();
    let id = db
        .query(&Query {
            families: &[Family::Name(FAMILY)],
            ..Query::default()
        })
        .expect("FAMILY names a bundled face");
    let face = db.face(id).expect("the id came from this database");

    assert_eq!(face.style, Style::Normal);
    assert!(
        face.families.iter().any(|(name, _)| name == FAMILY),
        "family list {:?} does not contain {FAMILY:?}",
        face.families
    );
}

#[test]
fn an_italic_query_resolves_to_the_italic_face() {
    let db = database();
    let upright = db
        .query(&Query {
            families: &[Family::Name(FAMILY)],
            ..Query::default()
        })
        .expect("upright face");
    let italic = db
        .query(&Query {
            families: &[Family::Name(FAMILY)],
            style: Style::Italic,
            ..Query::default()
        })
        .expect("italic face");

    assert_ne!(
        upright, italic,
        "the italic query must pick the second file"
    );
    assert_eq!(db.face(italic).expect("face").style, Style::Italic);
}

/// The text stack picks a face by *exact* declared weight, and falls back to
/// whatever other family declares it when the requested one does not. A
/// variable font declares a single weight, so without the copies
/// `Luminate::fonts` makes, asking the kit's family for anything but 400
/// silently rendered in a system font.
///
/// This is the regression: every weight the kit and its animations reach for
/// must resolve to a face of [`FAMILY`], declaring exactly that weight.
#[test]
fn every_declared_weight_resolves_within_the_family() {
    let mut db = Database::new();
    for font in iced_luminate::Luminate::fonts() {
        db.load_font_data(font.into_owned());
    }

    for weight in [400].into_iter().chain(DECLARED_WEIGHTS) {
        for (style, label) in [(Style::Normal, "upright"), (Style::Italic, "italic")] {
            let id = db
                .query(&Query {
                    families: &[Family::Name(FAMILY)],
                    weight: fontdb::Weight(weight),
                    style,
                    ..Query::default()
                })
                .unwrap_or_else(|| panic!("{label} {weight} matched nothing"));
            let face = db.face(id).expect("the id came from this database");

            assert!(
                face.families.iter().any(|(name, _)| name == FAMILY),
                "{label} {weight} left the family: {:?}",
                face.families
            );
            assert_eq!(
                face.weight,
                fontdb::Weight(weight),
                "{label} {weight} matched a face declaring {:?}; the text stack \
                 would prefer any other family that declares {weight} exactly",
                face.weight
            );
            assert_eq!(face.style, style, "{label} {weight}");
        }
    }
}

#[test]
fn the_default_font_requests_the_family() {
    assert_eq!(FONT.family, iced_luminate::iced::font::Family::Name(FAMILY));
    assert_eq!(FONT.style, iced_luminate::iced::font::Style::Normal);
}
