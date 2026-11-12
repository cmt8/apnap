//! The creature types (CR 205.3m (2026-11-06)) and the values a player can choose "as this
//! enters" or while a spell resolves: a creature type, a color (CR 105.4).

use crate::Color;
use serde::{Deserialize, Serialize};

/// CR 205.3m: every creature type, sorted (byte order, for binary search). The CR's curly
/// apostrophes are written straight, as type lines have them ("C'tan").
pub const CREATURE_TYPES: [&str; 367] = [
    "Advisor",
    "Aenar",
    "Aetherborn",
    "Alien",
    "Ally",
    "Andorian",
    "Android",
    "Angel",
    "Antelope",
    "Ape",
    "Archer",
    "Archon",
    "Armadillo",
    "Army",
    "Artificer",
    "Assassin",
    "Assembly-Worker",
    "Astartes",
    "Atog",
    "Aurochs",
    "Avatar",
    "Azra",
    "Badger",
    "Bajoran",
    "Balloon",
    "Barbarian",
    "Bard",
    "Basilisk",
    "Bat",
    "Bear",
    "Beast",
    "Beaver",
    "Beeble",
    "Beholder",
    "Berserker",
    "Betazoid",
    "Bird",
    "Bison",
    "Blinkmoth",
    "Boar",
    "Bolian",
    "Borg",
    "Brikar",
    "Bringer",
    "Brushwagg",
    "C'tan",
    "Caitian",
    "Camarid",
    "Camel",
    "Capybara",
    "Cardassian",
    "Caribou",
    "Carrier",
    "Cat",
    "Centaur",
    "Child",
    "Chimera",
    "Citizen",
    "Cleric",
    "Clown",
    "Cockatrice",
    "Construct",
    "Coward",
    "Coyote",
    "Crab",
    "Crocodile",
    "Custodes",
    "Cyberman",
    "Cyclops",
    "Dalek",
    "Dauthi",
    "Demigod",
    "Demon",
    "Denobulan",
    "Deserter",
    "Detective",
    "Devil",
    "Dinosaur",
    "Djinn",
    "Doctor",
    "Dog",
    "Dragon",
    "Drake",
    "Dreadnought",
    "Drix",
    "Drone",
    "Druid",
    "Dryad",
    "Dwarf",
    "Echidna",
    "Edosian",
    "Efreet",
    "Egg",
    "El-Aurian",
    "Elder",
    "Eldrazi",
    "Elemental",
    "Elephant",
    "Elf",
    "Elk",
    "Employee",
    "Eternal",
    "Excalbian",
    "Eye",
    "Faerie",
    "Ferengi",
    "Ferret",
    "Fish",
    "Flagbearer",
    "Fox",
    "Fractal",
    "Frog",
    "Fungus",
    "Gamer",
    "Gamma",
    "Gargoyle",
    "Germ",
    "Giant",
    "Giraffe",
    "Gith",
    "Glimmer",
    "Gnoll",
    "Gnome",
    "Goat",
    "Goblin",
    "God",
    "Golem",
    "Gorgon",
    "Gorn",
    "Graveborn",
    "Gremlin",
    "Griffin",
    "Guest",
    "Hag",
    "Halfling",
    "Hamster",
    "Harpy",
    "Hedgehog",
    "Hellion",
    "Hero",
    "Hippo",
    "Hippogriff",
    "Hirogen",
    "Homarid",
    "Homunculus",
    "Horror",
    "Horse",
    "Human",
    "Hydra",
    "Hyena",
    "Illusion",
    "Illyrian",
    "Imp",
    "Incarnation",
    "Inhuman",
    "Inkling",
    "Inquisitor",
    "Insect",
    "Jackal",
    "Jellyfish",
    "Jem'Hadar",
    "Juggernaut",
    "Kangaroo",
    "Kavu",
    "Kelpien",
    "Kirin",
    "Kithkin",
    "Klingon",
    "Knight",
    "Kobold",
    "Kor",
    "Kraken",
    "Kree",
    "Kwejian",
    "Lamia",
    "Lammasu",
    "Lanthanite",
    "Leech",
    "Lemur",
    "Leviathan",
    "Lhurgoyf",
    "Licid",
    "Lizard",
    "Llama",
    "Lobster",
    "Manticore",
    "Masticore",
    "Medusan",
    "Melkotian",
    "Mercenary",
    "Merfolk",
    "Metathran",
    "Minion",
    "Minotaur",
    "Mite",
    "Mole",
    "Monger",
    "Mongoose",
    "Monk",
    "Monkey",
    "Moogle",
    "Moonfolk",
    "Mount",
    "Mouse",
    "Mutant",
    "Myr",
    "Mystic",
    "Na'kuhl",
    "Nautilus",
    "Necron",
    "Nephilim",
    "Nightmare",
    "Nightstalker",
    "Ninja",
    "Noble",
    "Noggle",
    "Nomad",
    "Nymph",
    "Ocampa",
    "Octopus",
    "Officer",
    "Ogre",
    "Ooze",
    "Orb",
    "Orc",
    "Orgg",
    "Orion",
    "Otter",
    "Ouphe",
    "Ox",
    "Oyster",
    "Pangolin",
    "Peasant",
    "Pegasus",
    "Pentavite",
    "Performer",
    "Pest",
    "Phelddagrif",
    "Phoenix",
    "Phyrexian",
    "Pilot",
    "Pincher",
    "Pirate",
    "Plant",
    "Platypus",
    "Porcupine",
    "Possum",
    "Praetor",
    "Primarch",
    "Prism",
    "Processor",
    "Q",
    "Qu",
    "Rabbit",
    "Raccoon",
    "Ranger",
    "Rat",
    "Rebel",
    "Reflection",
    "Rhino",
    "Rigger",
    "Robot",
    "Rogue",
    "Romulan",
    "Sable",
    "Salamander",
    "Samurai",
    "Sand",
    "Saproling",
    "Satyr",
    "Scarecrow",
    "Scientist",
    "Scion",
    "Scorpion",
    "Scout",
    "Sculpture",
    "Seal",
    "Serf",
    "Serpent",
    "Servo",
    "Shade",
    "Shaman",
    "Shapeshifter",
    "Shark",
    "Sheep",
    "Shi'ar",
    "Siren",
    "Skeleton",
    "Skrull",
    "Skunk",
    "Slith",
    "Sliver",
    "Sloth",
    "Slug",
    "Snail",
    "Snake",
    "Soldier",
    "Soltari",
    "Sorcerer",
    "Spawn",
    "Specter",
    "Spellshaper",
    "Sphinx",
    "Spider",
    "Spike",
    "Spirit",
    "Splinter",
    "Sponge",
    "Spy",
    "Squid",
    "Squirrel",
    "Starfish",
    "Surrakar",
    "Survivor",
    "Symbiote",
    "Synth",
    "Talaxian",
    "Talosian",
    "Tamarian",
    "Tellarite",
    "Tentacle",
    "Tetravite",
    "Thalakos",
    "Tholian",
    "Thopter",
    "Thrull",
    "Tiefling",
    "Time Lord",
    "Tosk",
    "Toy",
    "Treefolk",
    "Tribble",
    "Trill",
    "Trilobite",
    "Triskelavite",
    "Troll",
    "Turtle",
    "Tyranid",
    "Unicorn",
    "Utrom",
    "Vampire",
    "Varmint",
    "Vau-N'Akat",
    "Vedalken",
    "Villain",
    "Volver",
    "Vorta",
    "Vulcan",
    "Wall",
    "Walrus",
    "Warlock",
    "Warrior",
    "Weasel",
    "Weird",
    "Werewolf",
    "Whale",
    "Wizard",
    "Wolf",
    "Wolverine",
    "Wombat",
    "Worm",
    "Wraith",
    "Wurm",
    "Xindi",
    "Yeti",
    "Zombie",
    "Zubera",
];

/// One creature type of [`CREATURE_TYPES`] (CR 205.3m): what "choose a creature type"
/// chooses. Serialized as its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatureType(u16);

impl CreatureType {
    /// The creature type called `name`, if it is one.
    pub fn named(name: &str) -> Option<CreatureType> {
        CREATURE_TYPES
            .binary_search(&name)
            .ok()
            .map(|i| CreatureType(i as u16))
    }

    pub fn name(self) -> &'static str {
        CREATURE_TYPES[self.0 as usize]
    }

    /// Every creature type, in [`CREATURE_TYPES`] order.
    pub fn all() -> impl Iterator<Item = CreatureType> {
        (0..CREATURE_TYPES.len() as u16).map(CreatureType)
    }
}

/// Is `name` a creature type (CR 205.3m)?
pub fn is_creature_type(name: &str) -> bool {
    CreatureType::named(name).is_some()
}

impl Serialize for CreatureType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for CreatureType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = alloc::string::String::deserialize(d)?;
        CreatureType::named(&name)
            .ok_or_else(|| serde::de::Error::custom("not a creature type (CR 205.3m)"))
    }
}

/// What a player is asked to choose ("as this enters, choose …", "choose … ." while a
/// spell or ability resolves).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChoiceOf {
    /// A creature type (CR 205.3m).
    CreatureType,
    /// One of the five colors (CR 105.4: never colorless, never "multicolored").
    Color,
}

/// A value that was chosen: an object's "chosen type" / "chosen color" (CR 607.2d).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Chosen {
    CreatureType(CreatureType),
    Color(Color),
}

impl Chosen {
    /// What kind of choice this answers.
    pub fn of(self) -> ChoiceOf {
        match self {
            Chosen::CreatureType(_) => ChoiceOf::CreatureType,
            Chosen::Color(_) => ChoiceOf::Color,
        }
    }
}

/// A selector's reference to a choice: "of the chosen type", "that aren't of the chosen
/// type", "of the chosen color". Resolved against the choice of the ability's own source
/// (or of the resolving spell or ability) where the selector is used; with no choice made
/// (CR 607.5a) "of the chosen ..." matches nothing and "not of the chosen type" everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OfChosen {
    Type,
    NotType,
    Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_types_are_sorted_and_found() {
        assert!(CREATURE_TYPES.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(
            CreatureType::named("Elf").map(CreatureType::name),
            Some("Elf")
        );
        assert!(is_creature_type("Time Lord") && is_creature_type("C'tan"));
        assert!(!is_creature_type("Forest") && !is_creature_type("Equipment"));
        let t = CreatureType::named("Zombie").unwrap();
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(s, "\"Zombie\"");
        assert_eq!(serde_json::from_str::<CreatureType>(&s).unwrap(), t);
        assert!(serde_json::from_str::<CreatureType>("\"Forest\"").is_err());
    }
}
