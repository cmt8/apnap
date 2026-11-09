//! Print the generated rules text of every self-contained card:
//! `cargo run -p harness --example card_text`.

fn main() {
    let cat = harness::catalog();
    for d in &cat.defs {
        let cost = d.mana_cost.as_ref().map_or(String::new(), |m| m.to_text());
        let pt = match (d.power, d.toughness) {
            (Some(p), Some(t)) => format!(" {p}/{t}"),
            _ => String::new(),
        };
        println!(
            "{} {cost} — {}{pt}",
            d.name,
            card_model::describe::type_line(d)
        );
        for l in card_model::describe::rules_text(d) {
            println!("    {l}");
        }
    }
}
