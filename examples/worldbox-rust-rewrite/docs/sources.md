# Where the mechanics came from

`worldforge` is a cleanroom reimplementation: no WorldBox files, no decompiled code, no extracted
assets. The rules are genre conventions plus what public documentation says. These are the sources
that shaped the simulation (read 2026-10-03).

## Villages and kingdoms

* **worldbox-sandbox-god-simulator.fandom.com — "Civilizations, Kingdoms, and Villages"**
  A race claims territory, builds a fireplace, then a town hall, then ~3–5 level-1 houses; villagers
  gather (chop trees, mine, farm); mine costs 10 stone + 5 wood; the town hall upgrades to a castle
  (max level 3) before houses upgrade; a village starts with a 4×4 territory and expands; the capital
  is the first village of a kingdom's colour and its leader becomes king; **Inspiration** turns a
  non-capital village into its own kingdom; **Friendship** forces peace, **Spite** forces war.
* **en.namu.wiki — WorldBox rules summary**
  Low-loyalty villages revolt (with Diplomacy off, loyalty is pinned at +1000); "Kingdom Expansion"
  sends settlers from villages above a population threshold; a rebellion sweeps 1–6 neighbouring
  villages; the default era is the age of hope; Divine Light clears disease/madness/infection.
* **Steam announcement 0.21.0 ("MegaBox")** — the ages system (hope, sun, dark, tears, moon, chaos,
  wonders, ice, ash, despair), alliances, plots/plans, clans/bloodlines, loyalty shown in the city
  window, foundation dates, trait-driven max age.

## Powers

* **the-official-worldbox-wiki.fandom.com — "Powers"**: ~374 powers across 8 tabs (Main; Unit; World
  Shaping; Noosphere and Life; Animals, Creatures and Monsters; Nature and Disasters; Destruction and
  Chaos; Other). The tabs are what `powers::PowerCategory` is modelled on; the count is not.
* **Steam store page (app 1206560)** — the power list used for the destruction set: lightning, tornado,
  acid rain, nuke, meteorite, plague, dragons, UFOs; creatures include demons, skeletons, zombies,
  tumors, cold ones and Crabzilla; procedural world generation.

## Races and creatures

* **worldbox-sandbox-god-simulator.fandom.com — "Dwarves"**: baseline civilized stats 200 hp, 22 damage,
  4 armor, 30 speed, 0% dodge, 80% accuracy, 50 attack rate, 0% crit, 4 diplomacy, miner trait; leaders
  and kings get special models; 6 house tiers and 3 town-hall tiers.

## How this maps onto the code

| Source idea | In `worldforge` |
|---|---|
| fireplace → town hall → houses | `village::build_priorities` |
| housing caps population | `Village::housing`, `village_growth_tick` |
| capital = first village, its leader becomes king | `World::found_kingdom`, `kingdom::kingdom_tick` |
| a village with a hall and enough people crowns itself | `kingdom::CROWN_POP` |
| low loyalty ⇒ rebellion that drags neighbours in | `World::rebel`, `kingdom::rebellion_tick` |
| settlers leave crowded villages | `World::send_settlers`, `SETTLER_POP` |
| ages shift climate, growth and loyalty | `ages::Age::{biome_bonus, fertility_rate, loyalty_bonus, …}` |
| Dwarf stat block | `races::Race::def()` |
| powers by tab | `powers::PowerCategory`, 49 powers |
| Crabzilla, dragons, UFOs as single huge units | `races::Category::Boss` |

Numbers that are *not* sourced (house costs, siege rate, war-declaration thresholds, banter in the
chronicle) are consistent inventions: they were tuned until a 100-year run looked like a WorldBox game
rather than a spreadsheet. See `MODLOG.md` for the three tuning rounds that got it there.
