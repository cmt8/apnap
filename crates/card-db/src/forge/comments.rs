//! Comment lines in Forge's card scripts (docs/forge/script-format.md, "Comments").
//!
//! Forge skips every line whose first character is `#` (CardRules.Reader.readCard), so a
//! comment never changes what a card does *in Forge*. But a comment is also where script
//! authors write down that a script is wrong or incomplete ("TODO: the second ability is
//! not implemented"), and invariant 15 says the compiler never drops what it doesn't
//! understand. So every comment is reviewed (D-078):
//!
//! - `HARMLESS`: about Forge's AI or internals, or a section marker. Ignored.
//! - `INCOMPLETE`: says or suggests that the script does not fully implement the card.
//!   Blocks like `SCRIPT_DIFFERS` (`script:differs from the card`).
//! - anything else is a comment nobody has read: the blocker `comment:unreviewed`.
//!
//! An entry ending in `*` matches any comment that starts with the text before it (long
//! comments are listed by their first 60 characters, so Forge fixing a typo late in a
//! sentence doesn't block the card); any other entry must match the whole comment. Texts
//! are compared after dropping the `#` and collapsing white space. When a Forge update
//! brings a new comment it blocks its card until someone reads it and adds it here.

/// The blockers the comment lines of one face raise.
pub fn blockers(comments: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for c in comments {
        let text = c.split_whitespace().collect::<Vec<_>>().join(" ");
        if INCOMPLETE.iter().any(|(p, _)| matches(p, &text)) {
            out.push("script:differs from the card");
        } else if !HARMLESS.iter().any(|p| matches(p, &text)) {
            out.push("comment:unreviewed");
        }
    }
    out
}

/// Why a comment is in `INCOMPLETE`, for the reader (the comment's own words).
pub fn why_incomplete(comment: &str) -> Option<&'static str> {
    let text = comment.split_whitespace().collect::<Vec<_>>().join(" ");
    INCOMPLETE
        .iter()
        .find(|(p, _)| matches(p, &text))
        .map(|(_, why)| *why)
}

fn matches(pattern: &str, text: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => text.starts_with(prefix),
        None => text == pattern,
    }
}

// Generated from the pinned Forge commit; reviewed by hand.
const HARMLESS: &[&str] = &[
    "--- VARIANT B ---",
    "--- VARIANT C ---",
    "--- VARIANT D ---",
    "--- VARIANT F ---",
    "--- VARIANTS ---",
    "AFLifeLost will be set by LoseLife",
    "AI Preference is needed to make the AI consider the ability.*",
    "AI can now use snow mana to pay for activated abilities.",
    "AI will exile all cards from its graveyard currently",
    "AI won't specifically put mana in pool to activate the ability",
    "AILogic tag not quite right?",
    "Add AILogic for Optional",
    "Add a fake trigger to display spell ability text ingame",
    "Amount$ All for Strionic Resonator",
    "Amount$ All | CopyOnce$ True for Strionic Resonator",
    "Branch to cast that card from hand",
    "Branch to search the library and cast that card",
    "Card doesn't make sense to run unless you have 2 or more opponents",
    "Change the script in order to be compatible with Jund, each*",
    "ChangeType is fine being \"Card\" because if it's on the battl*",
    "Clear tracking effect if the creature put onto the battlefie*",
    "Combine Hedron Alignments in 4 different zones, cap the coun*",
    "Don't use References, when the variable is set by StoreSVar",
    "Double addition to try to get up to 4",
    "Draw cards",
    "Empty Effect for Cost",
    "Even though the Effect is \"Permanent\", it's not really permanent",
    "Flash needs to persist onto the stack too, since the SpellCa*",
    "HOU Release Notes: if power less than 0 as the ability resol*",
    "Keep track of countered cards even while in other zones, so*",
    "LibraryPosition is zero indexed. So 1 is second from the top",
    "Losing Effect",
    "Make SVars for granting abilities and triggers on clones dis*",
    "Make Svars for granting abilities and triggers on clones dis*",
    "Max returns any number lower than or equally to the number f*",
    "Must be amended if they add more card types",
    "NOTE: The AI will not activate the Ultimate, but otherwise i*",
    "NOTE: The AI will not play around this effect, so it's only*",
    "NOTE: The AI will not stop making land drops after casting t*",
    "Necessary because even if you cast this as the first spell,*",
    "Needed to set ChosenPlayer to be ChosenCardController",
    "Order matters here, put counters first so clone doesn't have*",
    "Release notes indicate that this effect should work with Veh*",
    "Removed reminder text because it's too long for SA choice wi*",
    "Repeat flip",
    "Restriction logic will need to be added to Human declare att*",
    "Return the card as a normal trigger",
    "Set up main abilities",
    "Some cards may allow you to discard to other zones",
    "Sorry for the double negative. Players can only be attacked*",
    "TODO fix for shared turn modes like TwoHeadedGiant",
    "TODO the AffectedZone Stack is needed for now but should be*",
    "TODO: AILogic$ DoSacrifice is a placeholder which signals Ai*",
    "TODO: Add Ability$Cycling to Cycling cards for the purpose o*",
    "TODO: Ensure that the AI actually uses the ability, and uses*",
    "TODO: Figure out a way to make an AIPreference that would wo*",
    "TODO: For AI, this acts as a curse spell to lock the opponen*",
    "TODO: If shared turns are ever implemented that are needed f*",
    "TODO: Improve AI logic. Currently AI will sacrifice even if*",
    "TODO: Improve the AI for this",
    "TODO: Improve the AI to make it bounce its own locked creatu*",
    "TODO: Improve the mana ritual AI such that it does not \"misf*",
    "TODO: Improve the tutoring logic for the AI. Currently will*",
    "TODO: Just like with Sakura-Tribe Elder, the AI will sac it*",
    "TODO: Mana prediction for lands that produce additional mana*",
    "TODO: Most likely the ChooseGenericEffect code can be simpli*",
    "TODO: The AI can only use this as a form of buff. Maybe teac*",
    "TODO: The AI currently won't use cycling to counter the spel*",
    "TODO: The AI doesn't seem to use transmute, but otherwise us*",
    "TODO: The AI generally is able to use this card, but will ba*",
    "TODO: The AI is bad with the \"put two cards on the bottom of*",
    "TODO: The AI is not yet fully aware that this card can gener*",
    "TODO: The AI maximizes payment, even if it can't return anyt*",
    "TODO: The AI successfully uses this to save one of its creat*",
    "TODO: The AI uses this card well, but it only distributes al*",
    "TODO: The AI will currently search for the most expensive va*",
    "TODO: The AI will generally prefer placing two +1/+1 counter*",
    "TODO: The AI will pretty much always use this to draw cards*",
    "TODO: The AI won't activate the effect yet, but then again,*",
    "TODO: The AI won't block multiple creatures yet, it seems. C*",
    "TODO: The AI won't use the \"draw a card\" ability on this yet*",
    "TODO: This creature seems almost AI playable, except the AI*",
    "TODO: Tutoring in general can be improved to make this card*",
    "TODO: improve AI tutoring logic",
    "TODO: improve the logic when the AI wants to sac creatures",
    "TODO: maybe improve the AI logic for the trigger so that the*",
    "TODO: the AI can be improved for this, it'll currently only*",
    "TODO: the AI doesn't really target the opponent with the tap*",
    "TODO: the AI will currently not make Mist Dragon lose flying*",
    "TODO: update logic for the AI to play around this",
    "TODO: use some kind of Any logic",
    "Target a creature for three damage",
    "The AI currently won't understand that this ability is actua*",
    "The AI will use the upkeep mana for instants and stuff when able",
    "The DelayedTrigger is for all player, not just for each atta*",
    "The X cost won't be limited by the number of charge counters*",
    "The YouCtrl in the above count is relative to each player checked",
    "The following construct specifies that the AI always plays t*",
    "The hostcard of SVar X is the controlled card",
    "The hostcard of SVar Y is the controlled card",
    "The keyword added does nothing itself other than create a ke*",
    "The reroll effect does not work with planar dice currently",
    "This SVar will be overridden by AF LoseLife",
    "This code presumes that you are only ever allowed to exert c*",
    "This xPaid doesn't do anything, it's just needed to make Cos*",
    "Turn off the duration at the beginning of the next turn stat*",
    "Turn targeted creature or player into chosen",
    "Turn targeted player into chosen",
    "Use DirectRemembered to copy the LKI",
    "Went for MostProminentInGame logic to give the AI a possible*",
    "Winning Effect",
    "X Will get overwritten by Announce",
    "X Will get overwritten by Blight",
    "X will be the Converted Mana Cost of the target of Aether Mutation",
    "X will be the Converted Mana Cost of the target of Mutation",
    "X will be the converted mana cost of the target of Death Mutation",
];

const INCOMPLETE: &[(&str, &str)] = &[
    (
        "Departure from Oracle justified by https://magic.wizards.com*",
        "Departure from Oracle justified by https://magic.wizards.com/en/news/announcements/oracle-changes-2020-04-10, deeming it more fun that older funny Chicken cards have the Bird creature type instead.",
    ),
    (
        "Departure from the letter of Oracle necessary so this card g*",
        "Departure from the letter of Oracle necessary so this card gets/gains bonuses/maluses predicated on creature type.",
    ),
    (
        "Doesn't follow the strict wording of the Oracle text current*",
        "Doesn't follow the strict wording of the Oracle text currently. The return effect currently returns cards who attacked back to their owner's hand instead of your hand.",
    ),
    (
        "Doesn't interact properly with Myr Welder right now",
        "Doesn't interact properly with Myr Welder right now",
    ),
    (
        "EDH Silver unofficial errata*",
        "EDH Silver unofficial errata found at https://edhsilver.com/cards/uc/mbp/celestine-cave-witch/",
    ),
    (
        "Everything working but when no artifact is sacrificed Sacked*",
        "Everything working but when no artifact is sacrificed SackedCMC to be overridden to see if a sacrifice actually happened",
    ),
    (
        "It probably doesn't matter whether graveyards are exiled sim*",
        "It probably doesn't matter whether graveyards are exiled simultaneously with Silent Gravestone, but nevertheless, is there a way to do it?",
    ),
    (
        "Note: For convenience, it was determined that \"Armored\" wasn*",
        "Note: For convenience, it was determined that \"Armored\" wasn't striked through on the type line of this card due to an editing lapse. The striked-through parts of the card text were excised from the script for lack of a ",
    ),
    (
        "Remember Destroyed still isn't quite right since Hellfire ca*",
        "Remember Destroyed still isn't quite right since Hellfire cares about Replacement effects too",
    ),
    ("TODO", "TODO"),
    (
        "TODO: check ruling with Twinning Staff",
        "TODO: check ruling with Twinning Staff",
    ),
    (
        "TODO: ideally the tokens should be created simultaneously af*",
        "TODO: ideally the tokens should be created simultaneously after all the players have finished paying mana, but that's difficult to implement.",
    ),
    (
        "TODO: need EnchantedLKI because it isn't enchanted anymore i*",
        "TODO: need EnchantedLKI because it isn't enchanted anymore if this is sacrificed",
    ),
    (
        "Technically this should trigger once per opponent, not sure*",
        "Technically this should trigger once per opponent, not sure how to do that.",
    ),
    (
        "This calculation isn't considering the remembered player, on*",
        "This calculation isn't considering the remembered player, only the remembered cards",
    ),
    (
        "We'll need to improve the script at some stage, especially i*",
        "We'll need to improve the script at some stage, especially if we add Hunter of Eyeblights or Razorfin Abolisher.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Vec<&'static str> {
        blockers(&[text.to_string()])
    }

    #[test]
    fn reviewed_comments_pass_and_new_ones_block() {
        assert!(one("--- VARIANTS ---").is_empty());
        assert!(one("  --- VARIANTS ---  ").is_empty());
        assert_eq!(one("--- VARIANTS --- and more"), ["comment:unreviewed"]);
        assert_eq!(one("something nobody has read"), ["comment:unreviewed"]);
        assert_eq!(one("TODO"), ["script:differs from the card"]);
        assert!(why_incomplete("TODO").is_some());
        // A prefix entry matches a longer text, an exact one doesn't.
        assert!(one("TODO: Improve the tutoring logic for the AI. Currently will generally look for the most expensive castable thing in the library (changed)").is_empty());
    }

    #[test]
    fn no_entry_is_listed_twice_or_shadowed() {
        let all: Vec<&str> = HARMLESS
            .iter()
            .copied()
            .chain(INCOMPLETE.iter().map(|(p, _)| *p))
            .collect();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert!(!matches(a, b.trim_end_matches('*')), "{a:?} shadows {b:?}");
                }
            }
        }
    }
}
