---
name: i18n-translate
description: Translate user-facing strings from English into other languages in message catalogs (paraglide/i18next/ICU-style JSON). Use when asked to translate strings, add translations, fill in missing translations, or localize UI copy. Gathers context from usage sites, sibling keys, and existing terminology before translating.
---

# i18n-translate

## Goal

Make every supported language catalog **complete and consistent with `en.json`**: no missing keys, no broken placeholders, no invented terminology. A translated catalog is done when it has exactly the same keys as `en.json`, every `{placeholder}` preserved, and terms that match what the project already uses — not what a dictionary would produce.

The default job is **gap-filling**. If you are triggered with no explicit instruction — or only a vague one like "translate" — do not ask questions. Assume the full scope:

1. Every target language (`fr`, `es`, `pl`, `ru`, `tr`).
2. Every key missing from that catalog (run the key-parity command below).
3. Translate each missing key following the workflow, verify the catalogs, report what you did and what you flagged.

## Think before translating (per key)

Do not translate a string by reading it alone. For each key, reason through — out loud, before writing the translation:

- **What is this string?** Grep the usage site. Is it a button label (short, imperative), a page title (capitalized), a toast (plain, past-tense), a dialog question, a placeholder? The form decides length and tone.
- **What do the siblings say?** Read the other keys in the same prefix group in *all* languages. They establish the register and, more importantly, contain the existing translations of recurring terms. If `common_save` is already `Zapisz` in pl, your new key saying "save" must use `Zapisz`, not `Zapisywać`.
- **What can break?** Check for `{placeholders}` (copy exactly, reorder only if grammar demands), plural `match` categories (pl/ru need `few`/`many` added, tr collapses to `other`), and gender/number agreement with neighboring words.
- **What am I unsure about?** If the English is ambiguous or a translation is a genuine judgment call, flag it in the final report instead of silently guessing.

State your answers to these four questions for each key (or each batch of similar keys), then write the translation.

## Layout (Tempest)

- Catalogs: `Tempest.Launcher/messages/{en,fr,es,pl,ru,tr}.json` — `en` is source; targets `fr, es, pl, ru, tr`
- Tab indentation; keys grouped by prefix (`common_`, `lobby_`, …), roughly alphabetical within groups
- Plurals are nested blocks: `{ "declarations": [...], "selectors": [...], "match": { "countPlural=one": ..., "countPlural=other": ... } }`
- `src/lib/paraglide/` is generated — never hand-edit
- Never rewrite English strings; `en.json` is source of truth

## Verify (always)

```bash
# Key parity per touched file
comm -23 <(jq -r 'keys[]' messages/en.json | sort) <(jq -r 'keys[]' messages/pl.json | sort)
# Placeholder parity
diff <(grep -o '{[a-zA-Z]*}' messages/en.json | sort -u) <(grep -o '{[a-zA-Z]*}' messages/pl.json | sort -u)
# Valid JSON
jq empty messages/pl.json
```

Then `pnpm check` (paraglide regenerates + types resolve).

## Report

After the run: which languages got how many new keys, which terms you unified, and every string you flagged for a native speaker. If a catalog already had zero gaps, say so instead of inventing work.
