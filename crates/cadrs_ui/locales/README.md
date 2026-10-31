# Translating cadrs

The interface's texts come from the catalogs here, one a language (issue #178). English
(`en.ftl`) is the source: every key the app uses is in it. A key a language doesn't translate
shows in English, so a catalog can grow a few keys at a time.

## The files

Fluent's simple form, one entry a line:

```
# A note for translators: what the text is, where it shows.
share = Share
# { $count } is how many features the list has.
features-count = Features ({ $count })
```

- `key = text`. Keys are lowercase with `-`; never translate them.
- `{ $name }` is filled in by the app: keep it exactly, anywhere in the sentence.
- Lines starting `#` are notes; `##` heads a group.
- Translate the app's own words only. Names people give (documents, parts, features) are never
  translated.

## The workflow

1. **Draft.** A contributor, or an LLM given `en.ftl` with its notes, writes `<code>.ftl`. The
   files shipped so far are such drafts (each says so in its first line).
2. **Review.** A native speaker who knows CAD reads the draft beside `en.ftl` and fixes the
   wording and the terms (feature, sketch, extrude, fillet…: keep to what the CAD tools in that
   language say). Change the first line when it's reviewed.
3. **Check.** `cargo test -p cadrs_ui i18n` fails on a line that isn't `key = text`, a key
   `en.ftl` doesn't have, or placeholders that differ from English's, and lists the keys a
   language doesn't translate yet. CI runs it.

A new language: add `<code>.ftl`, a line in `LANGUAGES` (`src/i18n.rs`) and a variant in
`Language` (`cadrs_core::preferences`).

## The languages

German, French, Spanish, Italian, Dutch, Portuguese (Brazil), Russian and Afrikaans are offered
in Preferences › Language. They were chosen from where the people who have opened issues,
commented on, starred or forked the repository say they are (Brazil, Germany, China, France,
Spain, Italy, the Netherlands and Belgium, Japan, Korea…) and the languages most CAD users read.
Chinese (Simplified), Japanese and Korean have drafts too, but aren't offered yet: the interface
font (Inter) has no CJK glyphs, so they need a font of their own first.
