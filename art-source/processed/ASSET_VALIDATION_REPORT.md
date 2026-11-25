# Piku — Illustration Asset Validation Report

Generated for the complete **156-element Piku production illustration library** (`24 animations × 6 frames = 144 frames` + `12 static illustrations`).

---

## 1. Source Discovery & Transparency Audit

All 5 original files in `assets/new-illustartions/` were inspected programmatically and visually, and preserved untouched with byte-for-byte identical copies in `art-source/original/`.

| Map ID | Actual Filename | Format / Mode | Dimensions | Raw Alpha Range | Key Transparency Finding |
| --- | --- | :---: | :---: | :---: | --- |
| **Image A** | `Piku’s Kawaii Animation Sprite Sheet.png` (`U+2019`) | PNG / RGBA | `1774 × 887` | `0..255` | Clean transparent corners; `6,451` far-background pixels with residual `alpha 1..4` stripped. |
| **Image B** | `Piku’s Pastel Emotion Sticker Sprite Sheet.png` (`U+2019`) | PNG / RGBA | `1774 × 887` | `0..255` | Clean transparent corners; `33,298` far-background pixels with residual `alpha 1..8` stripped. |
| **Image C** | `Piku Sprout Animation Sprite Sheet.png` | PNG / RGBA | `1774 × 887` | `0..255` | Clean transparent corners; `81,266` far-background pixels with residual `alpha 1..9` stripped. |
| **Image D** | `Piku’s Pastel Sticker Animation Sheet.png` (`U+2019`) | PNG / RGBA | `1774 × 887` | `0..255` | Clean transparent corners; `57,685` far-background pixels with residual `alpha 1..11` stripped; Row 1 confetti reaches `y = 4`. |
| **Image E** | `Piku’s Static Illustration Sticker Sheet.png` (`U+2019`) | PNG / RGBA | `1536 × 1024` | `0..254` | Semi-transparent pale cream-green card background veil (`alpha 10..95`, ~779k px) + green card borders + bottom label pills. Flood-fill de-hazed and peak alpha normalized `246..254 → 255`. |

> **Filename note:** Four of the five source filenames contain a Unicode right single quotation mark (`’`, `U+2019`) rather than ASCII `'`. All tooling reads/writes the exact filenames without renaming originals.

---

## 2. Detected Labels & Content-Derived Extraction Map

### Animated Sprite Sheets (Images A–D)

Every sheet resolved cleanly into **6 horizontal row bands** and **7 horizontal groups per row** (1 left pill label + 6 left-to-right frames) with zero overlap between labels and frames. Vertical splits were placed at the minimum-density gap between rows (including the tight `y = 304..307` gap between `Yawn` and `Drag` in Image C, guarded by connected-component edge filtering).

| Sheet | Row | Detected Visual Label | Asset ID | App State ID | Row Y Range | Label X Range | Alignment Strategy |
| --- | :-: | --- | --- | --- | :---: | :---: | --- |
| Image A | 1 | `Idle` | `idle` | `idle` | `0..159` | `58..199` | Core body center + ground baseline (`y=216`) |
| Image A | 2 | `Blink` | `blink` | `blink` | `159..313` | `58..199` | Core body center + ground baseline |
| Image A | 3 | `Wave (Hello)` | `wave` | `wave` | `313..460` | `39..210` | Core body center + ground baseline |
| Image A | 4 | `Happy (Bounce)` | `happy` | `happy` | `460..609` | `37..214` | Core body center + row baseline (preserves hop) |
| Image A | 5 | `Sleep` | `sleep` | `sleep` | `609..743` | `47..207` | Core body center + row baseline (preserves melt) |
| Image A | 6 | `Click Reaction` | `click_reaction` | `click` | `743..887` | `26..219` | Core body center + row baseline (preserves squish/hop) |
| Image B | 1 | `Excited` | `excited` | `excited` | `0..172` | `87..273` | Core body center + row baseline (preserves jump & `\|\|`) |
| Image B | 2 | `Content` | `content` | `content` | `172..326` | `88..273` | Core body center + ground baseline |
| Image B | 3 | `Blush` | `blush` | `blush` | `326..468` | `88..273` | Core body center + ground baseline |
| Image B | 4 | `Curious` | `curious` | `curious` | `468..605` | `88..274` | Core body center + ground baseline |
| Image B | 5 | `Thinking` | `thinking` | `thinking` | `605..740` | `89..275` | Core body center + ground baseline |
| Image B | 6 | `Surprised` | `surprised` | `surprised` | `740..887` | `84..284` | Core body center + ground baseline |
| Image C | 1 | `Focused` | `focused` | `focused` | `0..162` | `83..282` | Core body center + ground baseline |
| Image C | 2 | `Yawn` | `yawn` | `yawn` | `162..307` | `83..281` | Core body center + ground baseline |
| Image C | 3 | `Drag` | `drag` | `drag` | `307..459` | `84..282` | Core body center + row baseline (preserves dangle) |
| Image C | 4 | `Working` | `working` | `working` | `459..597` | `84..282` | Full solid (Piku + laptop) center + ground baseline |
| Image C | 5 | `Break` | `break` | `break` | `597..735` | `84..282` | Core body center + ground baseline |
| Image C | 6 | `Late Night` | `late_night` | `late_night` | `735..887` | `83..283` | Full solid (blanket) center + ground baseline |
| Image D | 1 | `Celebration` | `celebration` | `celebration` | `0..165` | `76..299` | Core body center + row baseline (top `y=0` keeps confetti) |
| Image D | 2 | `Snack` | `snack` | `snack` | `165..315` | `77..285` | Core body center + ground baseline |
| Image D | 3 | `Tea` | `tea` | `tea` | `315..452` | `80..286` | Core body center + ground baseline |
| Image D | 4 | `Cloud` | `cloud` | `cloud` | `452..596` | `80..285` | Core body center + ground baseline |
| Image D | 5 | `Stretch` | `stretch` | `stretch` | `596..729` | `77..286` | Core body center + ground baseline |
| Image D | 6 | `Peek` | `peek` | `peek` | `729..887` | `79..286` | Right-wall edge lock (`x=192`) + ground baseline |

### Static Illustration Sheet (Image E)

Two card sections (`UI States (6)` at `y = 230..470`, `Icons & Badges (6)` at `y = 678..908`), each cleanly partitioned across 6 horizontal zero-density valleys (`23–66 px` wide) excluding the top header, section badges, green rounded borders, and bottom pill labels.

| Section | Slot | Detected Visual Label | Asset ID | Category Dir | Source X Range | App Usage |
| --- | :-: | --- | --- | --- | :---: | --- |
| UI States (6) | 1 | `Empty State` | `empty_state` | `ui-states` | `28..292` | Active in `TodayView` & `HistoryView` empty state cards |
| UI States (6) | 2 | `Loading` | `loading` | `ui-states` | `292..522` | Library asset (reserved; existing views use compact skeletons) |
| UI States (6) | 3 | `Success` | `success` | `ui-states` | `522..766` | Library asset (reserved; no extra UI added) |
| UI States (6) | 4 | `Error` | `error` | `ui-states` | `766..1007` | Active in `TodayView` & `HistoryView` error state cards |
| UI States (6) | 5 | `No Connection` | `no_connection` | `ui-states` | `1007..1252` | Library asset (Piku is local-first/offline; reserved) |
| UI States (6) | 6 | `Not Found` | `not_found` | `ui-states` | `1252..1508` | Library asset (reserved) |
| Icons & Badges (6) | 1 | `Checkmark` | `checkmark` | `badges` | `28..267` | Library badge asset (reserved; no badge feature added) |
| Icons & Badges (6) | 2 | `Star / Favorite` | `favorite` | `badges` | `267..506` | Library badge asset (reserved; no feature added) |
| Icons & Badges (6) | 3 | `Bell / Notification` | `notification` | `icons` | `506..756` | Library icon asset (reserved; no feature added) |
| Icons & Badges (6) | 4 | `Calendar / Schedule` | `calendar` | `icons` | `756..1015` | Library icon asset (reserved) |
| Icons & Badges (6) | 5 | `Settings` | `settings` | `icons` | `1015..1240` | Library icon asset (reserved) |
| Icons & Badges (6) | 6 | `Achievement` | `achievement` | `badges` | `1240..1508` | Library badge asset (reserved; no achievements/streaks added) |

---

## 3. Automated & Visual Validation Checklist

| Check | Status | Details |
| --- | :---: | --- |
| All 24 animation directories exist | PASS | `art-source/processed/animations/<id>/` and `public/assets/character/piku/<id>/` |
| Exactly 6 frames per animation | PASS | `24 × 6 = 144` frames, strictly preserving left-to-right frame order (`_01`..`_06`) |
| All 12 static illustrations accounted for | PASS | `6 ui-states` + `3 icons` + `3 badges` in `art-source/processed/static/` |
| Transparent PNG output (`RGBA`) | PASS | `156/156` verified `RGBA`, corners strictly `alpha = 0` |
| No clipped character elements | PASS | `0` frames encroach on the `4 px` outer safe border (`0` clipping errors) |
| No unwanted background pixels or white halos | PASS | Visually verified on both Cloud Cream `#F8F6EF` and Dark Slate `#18181B` contact sheets |
| Consistent canvas size & alignment | PASS | All `156` assets standardized to `256 × 256`; animations extracted at `1:1` native pixel crispness aligned to `anchor = (0.5, 0.844)` (`y = 216`); verified via onion-skin previews |
| Manifest paths resolve correctly | PASS | Both `art-source/processed/manifest.json` (`156` assets) and `public/assets/character/piku/manifest.json` (`144` runtime frames) pass `validate_assets.py` with `0` errors |
| Application references & build | PASS | `pnpm run typecheck` (`tsc --noEmit`) and `pnpm run build` (`vite build`) pass with `0` errors |
| Original artwork preserved untouched | PASS | All 5 files in `assets/new-illustartions/` remain unmodified and duplicated into `art-source/original/` |

---

## 4. Source Artwork Observations (Reported Honestly — Never Altered)

- **`curious` Frame 5 (Image B, Row 4):** The source artwork contains a very faint smudge immediately behind Piku's left sprout leaf. Per preservation rules, the original artwork was not redrawn or edited.
- **`celebration` Frame 4 (Image D, Row 1):** The source artwork includes a small soft oval ground shadow beneath the airborne jump ticks; preserved faithfully.
- **Image E static illustrations:** Each source illustration includes a subtle warm-beige contact shadow directly under Piku's feet/props; preserved faithfully while stripping 100% of the pale-green card background veil.
- **File size notes (non-blocking):** Three rich frames (`late_night_01.png` at `35.3 KB`, `late_night_02.png` at `35.2 KB`, `celebration_04.png` at `38.2 KB`) slightly exceed the `35.0 KB` soft target due to detailed plaid blanket and multi-color confetti pixels. Total runtime pack is `3.71 MB` across all 144 frames; `PetWindow` preloads only the 9 core interactive/lifecycle states (54 frames) and lazy-loads the other 15 ambient/expression states on demand, while `GreetingPanel` loads only 4 states (`idle`, `blink`, `wave`, `happy`) and History/Preferences load `0` animation sprites.

---

## 5. Contact Sheets Generated for Visual Review

- **Individual animation contact sheets (24):** `art-source/processed/contact-sheets/animations/<id>_contact.png` (Light row + Dark row + Onion-Skin alignment column)
- **Grouped overview sheets (8):** `art-source/processed/contact-sheets/overviews/image_{a,b,c,d}_{light,dark}.png`
- **Static dual-backdrop sheet (1):** `art-source/processed/contact-sheets/overviews/image_e_static_dual.png`
