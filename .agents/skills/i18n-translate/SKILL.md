---
name: i18n-translate
version: 1.0.0
description: Translate user-facing strings from English into other languages in message catalogs (paraglide/i18next/ICU-style JSON). Use when asked to translate strings, add translations, fill in missing translations, localize UI copy, or audit a catalog for missing or broken translations. Gathers context from usage sites, sibling keys, and existing terminology before translating, and catches placeholder breakage, plural-category gaps, and invented terminology that make a translation read as machine output.
---

# i18n-translate: fill and repair message catalogs

You are a translator responsible for making every language catalog in this project complete, consistent, and indistinguishable from a translation done by someone who works on this product. This is not a general translation task — a translation that is linguistically correct but ignores the catalog's own established terms, drops a placeholder, or gets the plural form wrong is still broken, even though every word is "right."

## Your Task

When given a translation job — a specific key, a batch, or (the default) "fill whatever is missing":

1. **Find the gap.** If no keys are named, run the key-parity check below for every target language and treat every missing key as in scope.
2. **Gather context per key** — usage site, sibling translations, placeholders, plural rules. See Terminology Calibration below.
3. **Draft the translation**, then run it through the pattern checks in this file.
4. **Verify mechanically** (key parity, placeholder parity, valid JSON, `pnpm check`) before calling anything done.
5. **Report** what was translated, which terms you unified, and what you are not confident about.

If you are triggered with no explicit instruction, or a vague one like "translate," do not ask clarifying questions. Assume the full default scope: every target language (`fr`, `es`, `pl`, `ru`, `tr`), every key missing from `en.json`.

## Terminology Calibration (read before translating)

Before writing a single word of a translation, answer these for the key (or batch of similar keys):

- **What is this string, structurally?** Grep the usage site. A button label, a page title, a toast, a dialog question, and a placeholder all take a different length and grammatical mood — this decides your draft, not the English wording alone.
- **What do the siblings already say?** Read every other key under the same prefix, in every language, before inventing a translation. If `common_save` is already `Zapisz` in `pl`, a new key that means "save" reuses `Zapisz`. It does not become `Zapisywać` because that also technically means "save."
- **What can break mechanically?** Placeholders (`{count}`, `{name}`), plural `match` categories (`pl`/`ru` need `few`/`many`; `tr` collapses to `other`), and grammatical agreement with neighboring words.
- **What am I actually unsure about?** If the English is ambiguous, or a call is genuinely a judgment call between two valid translations, say so in the final report instead of silently picking one.

## Project Layout (Tempest)

- Catalogs: `Tempest.Launcher/messages/{en,fr,es,pl,ru,tr}.json` — `en` is the source of truth; targets are `fr, es, pl, ru, tr`.
- Tab indentation; keys grouped by prefix (`common_`, `lobby_`, …), roughly alphabetical within each group.
- Plurals are nested blocks: `{ "declarations": [...], "selectors": [...], "match": { "countPlural=one": ..., "countPlural=other": ... } }`.
- `src/lib/paraglide/` is generated. Never hand-edit it — see pattern 8 below.
- `en.json` is the source. Never rewrite English strings while translating — see pattern 8.

---

## CONTEXT PATTERNS

### 1. Translating a string without reading its usage site

**Problem:** The same English word takes different forms depending on what it labels. Translating from the string alone, without checking whether it is a button, a heading, or a toast, produces grammatically valid text that is wrong for its slot.

**Before (fr, `lobby_leaveButton`, translated blind):** `"Départ"` — a noun, reads like a train-station sign.

**After (checked usage: it's a button label):** `"Quitter"` — matches the imperative verb form every other button in the catalog uses.

### 2. Ignoring sibling keys and reinventing terminology

**Problem:** The catalog already has an established word for a concept. Picking a synonym instead of reusing it — even a perfectly good synonym — makes the UI read as if it were translated by three different people.

**Before (pl):** `common_save` is already `"Zapisz"`. A new key `profile_saveChanges` is translated as `"Zachowaj zmiany"`.

**After:** `"Zapisz zmiany"` — reuses the established verb.

### 3. Calquing English word order

**Problem:** Carrying English syntax into a language that structures the sentence differently produces text a native speaker can parse but would never write.

**Before (fr):** "Are you sure you want to delete this item?" → `"Êtes-vous sûr vous voulez supprimer cet article?"` (missing the required `que`, and `article` is the wrong register for a UI item).

**After:** `"Êtes-vous sûr de vouloir supprimer cet élément ?"`

---

## MECHANICAL PATTERNS

### 4. Dropped or renamed placeholders

**Problem:** `{placeholder}` tokens are code, not prose. Deleting one during translation, or "translating" its name, breaks string interpolation at runtime — the app is passing a value by that exact name.

**Before (es, `common_itemsLeft`):** en `"{count} items left"` → `"artículos restantes"` (placeholder dropped) or `"{cantidad} artículos restantes"` (placeholder renamed).

**After:** `"{count} artículos restantes"` — same token, exact spelling, moved to fit Spanish word order if grammar requires it.

### 5. Plural category collapse

**Problem:** English only distinguishes `one`/`other`. Polish and Russian need `few` and `many` as well; Turkish collapses everything to `other`. Filling only the categories English has leaves the app showing the wrong text for counts like 2–4 in Polish.

**Before (pl, only `one`/`other` filled):** 3 nowych wiadomości falls through to the `other` block meant for 5+, and reads slightly off.

**After:** add the `few` category: `"few": "{count} nowe wiadomości"`, matching Polish's four-way plural split.

### 6. Gender or number agreement drift

**Problem:** An adjective translated correctly in isolation can still disagree in gender or number with the noun it modifies once dropped into a sentence, especially when the same base word is reused as both noun and adjective elsewhere in the catalog.

**Before (ru):** `"новый"` (masculine) applied to a feminine noun it now modifies.

**After:** `"новая"` — agreement checked against the actual noun in this string, not the word's default form.

---

## STRUCTURAL PATTERNS

### 7. Broken key parity

**Problem:** A translated key that doesn't exactly match a key in `en.json` — renamed, re-cased, or restructured — passes a naive glance but fails the parity check and leaves an orphaned string plus a silently missing translation.

**Before:** `en.json` has `lobby_delete_room_confirm`; the translation is added under `lobby_deleteRoom_confirmMsg`.

**After:** Match the exact key name, character for character.

### 8. Rewriting the English source or the generated output

**Problem:** Noticing the English is awkward and "fixing" `en.json` while translating changes the source of truth for every language at once, silently, as a side effect of an unrelated task. Editing `src/lib/paraglide/` directly is worse — it's regenerated and the edit disappears.

**Before:** Translator adjusts `en.json`'s wording to be clearer, then translates the new wording.

**After:** Leave `en.json` untouched. If the English is genuinely wrong, flag it in the report instead of silently changing it.

### 9. Inventing or dropping keys mid-batch

**Problem:** Splitting one English key into two translated ones (or merging two into one) because it "reads better" in the target language breaks the catalog's one-to-one key mapping that the app relies on.

**Before:** en has a single `error_networkTimeout`; the pl translation is split into `error_networkTimeout_title` and `error_networkTimeout_body`.

**After:** One translated string per English key, however the sentence needs to be built internally.

---

## TONE AND REGISTER PATTERNS

### 10. Formality mismatch with sibling keys

**Problem:** Switching between formal and informal address (vous/tu, Sie/du, formal/informal ty) partway through a catalog reads as if the UI can't decide who it's talking to.

**Before (fr, casual lobby UI where siblings use `tu`):** `"Voulez-vous vraiment quitter ?"`

**After:** `"Tu veux vraiment partir ?"` — matches the register the rest of the lobby copy already uses.

### 11. Overtranslation — adding information the source doesn't have

**Problem:** "Helpfully" clarifying a short English string by adding scope or detail not present in the source changes what the string promises, not just how it sounds.

**Before:** en `"Delete"` → fr `"Supprimer définitivement ce fichier"` (adds "permanently" and "this file," neither of which is in the source or guaranteed true for every place this key is used).

**After:** `"Supprimer"`.

### 12. Dictionary-literal word choice over domain terminology

**Problem:** The most common dictionary translation of a word is not always the one the domain uses. Grep how the product's genre already names the concept before defaulting to a textbook synonym.

**Before (es, game lobby):** `"Room"` → `"Habitación"` (bedroom-flavored).

**After:** `"Sala"` — the term used across game-lobby UIs in Spanish.

---

## DETECTION GUIDANCE

### What NOT to flag as a problem

A correct, idiomatic translation can look "different" from the English in ways that are not errors. Before touching a translation, check that you aren't about to break something that was already right:

- **Different length.** Languages are not the same length as English for the same meaning. A German string running twice as long as its English source is normal, not a sign of overtranslation.
- **Reordered placeholders.** `{name}` appearing at the start of a Turkish sentence when it was mid-sentence in English is correct if Turkish grammar puts it there — the token must be preserved, not its position.
- **Missing formality distinction.** Languages without a formal/informal split (like English itself) don't need one invented for them.
- **A term that looks like a false friend but isn't.** Some words that resemble English cognates are the actual correct term in context (French `contrôler` meaning "to check," not "to control," is standard usage, not an error, when that's genuinely what's meant).
- **A shorter translation than expected.** Some languages compress meaning more densely than English; a terse translation isn't automatically an undertranslation.

### Signs of a correct translation (preserve these)

- **Exact reuse of an established glossary term**, even where a fresher synonym exists.
- **A plural block with every CLDR category the target locale requires**, not just the ones English has.
- **Register that matches the surrounding keys** in the same prefix group, not the friendliest or most formal option in isolation.
- **Placeholder tokens copied byte-for-byte from the English source**, regardless of where they land in the sentence.

---

## Process and Output

1. Run the key-parity check for each target language; that list is your work queue.
2. For each key, answer the four Terminology Calibration questions, then write a **draft translation**.
3. Ask: **"What would a native speaker on this team immediately flag in the draft below — wrong register, invented term, broken placeholder, wrong plural form?"** Answer briefly.
4. Revise into the **final translation** addressing anything the audit surfaced.
5. Run the verification commands (below) against every touched file.

### Verify (always, after any edit)

```bash
# Key parity per touched file
comm -23 <(jq -r 'keys[]' messages/en.json | sort) <(jq -r 'keys[]' messages/pl.json | sort)
# Placeholder parity
diff <(grep -o '{[a-zA-Z]*}' messages/en.json | sort -u) <(grep -o '{[a-zA-Z]*}' messages/pl.json | sort -u)
# Valid JSON
jq empty messages/pl.json
```

Then `pnpm check` (paraglide regenerates and types resolve).

### Report

State which languages got how many new keys, which terms you unified across the catalog, and every string you're flagging for a native speaker to double-check. If a catalog already had zero gaps, say that — don't invent work to look busy.

---

## Full Example

**Task:** fill the missing `pl` translation for `lobby_deleteRoom_confirm`.

**en.json:** `"lobby_deleteRoom_confirm": "Are you sure you want to delete {roomName}? This can't be undone."`

**Calibration:**
- *Structurally:* grep shows this feeds a confirmation dialog body, not a button — full sentence, question form.
- *Siblings:* `lobby_leaveRoom_confirm` in pl already uses informal `Czy na pewno chcesz…?` and the second-sentence pattern `Tej operacji nie można cofnąć.` — reuse both.
- *Mechanical:* one placeholder, `{roomName}`, must survive unchanged.
- *Unsure about:* nothing here — this one's a clean pattern match on an existing sibling.

**Draft:** `"Czy na pewno chcesz usunąć {roomName}? Tej operacji nie można cofnąć."`

**Audit — what would a native speaker flag?** Nothing: register matches `lobby_leaveRoom_confirm`, the placeholder is untouched, and the sentence structure is idiomatic Polish rather than a calque of the English "This can't be undone."

**Final:** unchanged from the draft.

**Verify:**
```bash
$ comm -23 <(jq -r 'keys[]' messages/en.json | sort) <(jq -r 'keys[]' messages/pl.json | sort)
# lobby_deleteRoom_confirm no longer appears — parity restored
$ jq empty messages/pl.json && echo "valid JSON"
valid JSON
```

**Report:** Added 1 key to `pl` (`lobby_deleteRoom_confirm`). Reused the confirmation-dialog phrasing already established by `lobby_leaveRoom_confirm`. Nothing flagged for review.

---

## Reference

Plural category rules per locale follow the [CLDR Language Plural Rules](https://www.unicode.org/cldr/cldr-aux/charts/30/supplemental/language_plural_rules.html) — check a locale's exact category list there before deciding a `few`/`many` block is unnecessary.