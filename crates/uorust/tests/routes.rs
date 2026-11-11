//! `UE-05`: the engine's router and `model/routes.toml` agree, route for route
//! and vector for vector.

use std::path::PathBuf;

fn table() -> toml::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../model/routes.toml");
    std::fs::read_to_string(&path)
        .expect("model/routes.toml")
        .parse()
        .expect("routes.toml parses")
}

fn field<'a>(row: &'a toml::Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn routes_agree_ue_05() {
    let table = table();
    let rows = table
        .get("route")
        .and_then(toml::Value::as_array)
        .expect("routes");
    assert_eq!(rows.len(), uorust::ROUTES.len(), "route count");
    for (row, route) in rows.iter().zip(uorust::ROUTES) {
        assert_eq!(field(row, "id"), route.id);
        assert_eq!(field(row, "rust_fact"), route.rust_fact, "{}", route.id);
        assert_eq!(
            field(row, "prism_record"),
            route.prism_record,
            "{}",
            route.id
        );
        assert_eq!(field(row, "facet"), route.facet, "{}", route.id);
    }
    // Every vector of the router's wire form; the `ui-` requests are the
    // application's workbench envelope around it, which the engine does not
    // speak.
    let vectors = table
        .get("vector")
        .and_then(toml::Value::as_array)
        .expect("vectors");
    let mut replayed = 0;
    for vector in vectors {
        let request = hex(field(vector, "request_hex"));
        if request.starts_with(b"ui-") {
            continue;
        }
        assert_eq!(
            uorust::respond(&request),
            field(vector, "response"),
            "{}",
            field(vector, "case")
        );
        replayed += 1;
    }
    assert!(
        replayed > uorust::ROUTES.len(),
        "every route and every refusal"
    );
}

#[test]
fn a_planted_disagreement_is_caught_ue_05() {
    // The defect the gate exists for: a router that routes a refused fact.
    assert_eq!(
        uorust::respond(b"error-enum\nstate-enum"),
        "refused ambiguous\nfact error-enum\nfact state-enum"
    );
    assert_ne!(uorust::respond(b"struct"), uorust::respond(b"pub-fn"));
    assert_eq!(
        uorust::respond(b"struct\n"),
        "refused unknown-fact\nfact struct"
    );
}
