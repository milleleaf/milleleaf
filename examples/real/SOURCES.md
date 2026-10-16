# Real-world Office fixtures

Three real files published by UK public bodies on GOV.UK. They were made in Microsoft Office, not
generated, and are used to find what Milleleaf loses on import and export. They were downloaded
on 2026-10-16 and are byte-identical to the published files (SHA-256 below). None has been edited.

**Licence, all three:** every GOV.UK publication page carries this footer: "All content is
available under the Open Government Licence v3.0, except where otherwise stated"
(https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/). Neither the three
pages nor the files state otherwise: the page bodies and all of the XML text were searched for
copyright, licence, © and Crown, and nothing contradicted the footer. OGL v3 allows copying,
publishing, distributing and adapting the material, including commercially, with attribution.
**Caveat:** OGL v3 does not cover departmental or public-body logos and crests. The Ofsted logo
in `board-report.docx` and the Ofqual template imagery in `briefing.pptx` are not licensed by it.
Keeping them in a private test repo is low risk. Strip or replace them before redistributing any
derived output publicly.

**Attribution:** Contains public sector information licensed under the Open Government Licence v3.0.

**How the counts were made:** each file was opened with `python3 -I` and `zipfile` plus
`xml.etree`. Nothing was executed and nothing was opened in Office. The personal-data scan ran
email and UK-phone regexes over the text **and the attribute values** of every XML part.
Scanning attributes matters: one rejected candidate hid a staff email address in `word/people.xml`.

---

## board-report.docx

| | |
|---|---|
| Title | Ofsted Board meeting minutes 7 February 2017 |
| Publisher | Ofsted (Office for Standards in Education, Children's Services and Skills) |
| Source URL | https://assets.publishing.service.gov.uk/media/5a82423240f0b6230269baef/Board_Minutes_070217.docx |
| Linking page | https://www.gov.uk/government/publications/ofsted-board-meetings-2017-papers-and-minutes ("Ofsted Board meetings 2017: papers and minutes") |
| Date | meeting 2017-02-07. Page first published 2017-02-20, updated 2018-01-26. File modified 2017-10-20 (core.xml) |
| Licence | OGL v3, from the GOV.UK footer on the linking page |
| SHA-256 | `474118d241338cf333c65bc710d98e6b8d691fa734f41f6b2c72c0e83b5257c2` |
| Size | 68,262 bytes |
| Application | Microsoft Office Word, AppVersion 15.0000 (Word 2013), template Normal |

**Inventory:** about 5 pages (4 rendered page breaks; app.xml wrongly says `Pages=1`). 92
paragraphs. 1 table: 3 grid columns, 6 rows, 5 `w:vMerge` cells, no `gridSpan`. 42 paragraphs
with `w:numPr`; numbering.xml has 18 abstractNums and 19 nums, in decimal and bullet formats.
1 image: a JPEG logo in `header3.xml`, the first-page header (`titlePg`). 3 headers and 3 footers
in 1 section. 33 styles are defined, but **no paragraph style is used in the body**: headings
are direct formatting (486 `w:rFonts`, 98 `w:b`). 1 leftover tracked insertion (`w:ins`). Also
present: footnotes.xml and endnotes.xml (empty), people.xml (a Windows SID, no email), 5
SharePoint customXml parts and custom.xml. No charts, fields, hyperlinks, comments or macros.

**Personal data scan:** 0 real hits. 1 false positive: a number inside styles.xml. The body names
board members and staff in their official roles (for example HMCI and the Chief Operating
Officer), which the brief allows. Metadata: `dc:creator` is a generic "London010". The
lastModifiedBy and the tracked-change author are Ofsted staff names, with no contact details.

**Why it's messy:** a real board paper typed with direct formatting instead of styles. The logo
lives only in the first-page header. An action table uses vertical merges. Numbering is spread
over 19 list instances. A tracked change was published by accident. The core.xml title
("DRAFT Ofsted Board minutes 10 June 2014 - draft") is stale, left over from a reused template.
Shortfall against the brief: no `gridSpan` merges and no body images.

## finance.xlsx

| | |
|---|---|
| Title | Reference Class Forecasting Workbook |
| Publisher | Homes England |
| Source URL | https://assets.publishing.service.gov.uk/media/65ca3b6f14b83c000ea716f0/Reference_Class_Forecasting_Workbook.xlsx |
| Linking page | https://www.gov.uk/government/publications/optimism-bias-and-contingency-at-homes-england ("Measuring Social Value Paper 3: Optimism Bias and Contingency at Homes England") |
| Date | page first published 2024-02-08, updated 2024-02-12. File created 2020-08-17, modified 2024-02-12 |
| Licence | OGL v3, from the GOV.UK footer on the linking page |
| SHA-256 | `41192a64e3cd159769aa72a837021cc59f0ce248f171629011f862fae5735738` |
| Size | 849,109 bytes |
| Application | Microsoft Excel, AppVersion 16.0300 |

**Inventory:** 11 sheets. 3 are visible (Cover Sheet, Input, Output) and **8 are hidden**: RCF
Calc, Cost RCF (MMC), RCF SOBC/OBC/FBC/CCT data, Selectors, and "Asset classes - discussed".
43,462 cells. **25,932 formulas**, 9,565 of them cross-sheet. Functions: IF 23,830, VLOOKUP 278,
SUM 53, SUMPRODUCT 14, AVERAGE 13, CONCATENATE 8, OFFSET 6, MATCH 6, RIGHT 5, FORECAST 3,
SUBTOTAL 1. **444 merged ranges.** 3 custom number formats: two accounting formats and `0.0%`.
The built-in formats in use include percent (9/10), accounting (43), dates (14/15/17) and
thousands (3). 803 cellXfs, 21 fills, 144 borders. 2 conditional-formatting blocks and 4 data
validations. **9 native charts** (1 pie, 8 line) on 8 drawings, plus 2 PNG images. 7 comment
parts with legacy VML. 20 defined names, including one that resolves to `Input!#REF!`, one that
points into the external workbook, and `_Hlk…` bookmark names pasted in from Word. **1 external
link** to another workbook on a UNC path (a network folder Cost Estimate - 240622 MB.xlsx`).
No macros, pivot tables or Excel tables.

**Personal data scan:** 0 hits. Comment authors are two named individuals ("Mr Bartlett" and
"Sam Franzen"), and `dc:creator`/`lastModifiedBy` are two more names. These are the
consultants and officials who built the model, with no contact details. The external-link path
contains a hashed home-directory ID, not a name.

**Why it's messy:** a production model that has been extended for years. Most of its logic sits
in hidden sheets, with tens of thousands of nested IFs. It has a broken defined name and a stale
link to someone's network drive. Merged headers and accounting/percent formats sit next to native
charts. Shortfall: no explicit £ currency format (accounting formats only), and it is a forecasting
model rather than an invoice. The runner-up for a £-heavy workbook is listed below.

## briefing.pptx

| | |
|---|---|
| Title | GCSE, AS and A level reforms in England — slide pack (updated May 2019) |
| Publisher | Ofqual, with the Department for Education |
| Source URL | https://assets.publishing.service.gov.uk/media/5cd56c51e5274a3fd79d3727/OfqualDfESlidepack_2019.pptx |
| Linking page | https://www.gov.uk/government/publications/qualifications-reform-resources-for-teachers ("Qualifications reform: resources for teachers"). The page says: "This slide pack has been produced by Ofqual, in collaboration with the Department for Education" |
| Date | page first published 2016-07-13, updated 2019-05-10. File modified 2019-05-10 |
| Licence | OGL v3, from the GOV.UK footer on the linking page |
| SHA-256 | `040a0eeb7421cc68ecbf9816f7f964bb1db7e561aef8f0ee8fad70a78e6219ca` |
| Size | 2,724,893 bytes |
| Application | Microsoft Macintosh PowerPoint, AppVersion 16.0000, template "Ofqual Powerpoint Template" |

**Inventory:** 28 slides. **28 notes slides, all of them with text.** 2 slide masters and 30
layouts. 2 native tables (slides 6 and 7) and 8 picture shapes on slides. 24 media files (20 JPEG,
4 PNG); most are branded background images on the layouts. A stale `commentAuthors.xml` lists 4
authors, but no comment parts remain. 4 SharePoint customXml parts. The core.xml title is "Hints
& tips", left from a reused template. No native charts, SmartArt, OLE objects or macros.

**Personal data scan:** 4 hits, all organisational contact points, not individuals: the
public.enquiries@ofqual.gov.uk mailbox (slide 28 and its rels) and the public helpline numbers
0370 000 2288 and 0300 303 3344. commentAuthors and core.xml give civil servants' names
without contact details.

**Why it's messy:** made on Mac PowerPoint from a departmental template. Branding sits on 30
layouts across 2 masters. Every slide has speaker notes. It contains tab-aligned pseudo-tables
as well as real tables, and leftover comment authors and template metadata. **Shortfall: it has no native
chart.** Chart round-tripping is covered by the 9 native charts in `finance.xlsx`. If a deck with
charts is needed, see the runner-up below.

---

## Candidates looked at and not chosen

Rejected for personal data (individual work email addresses):

- **Leman Foxtrot Rig Site Preparation, Appropriate Assessment** (DESNZ/OPRED, 2026, docx, 32 pp).
  It was the richest DOCX: 19 tables, 15 `gridSpan`, TOC fields, 6 sections, an unresolved comment,
  a `[trash]/` part and a sensitivity label. Rejected because `word/people.xml` holds an
  official's personal work email address in the `w15:userId` attribute.
- **Ofqual "Looking ahead to results days" webinar** (2023, pptx). It has a native chart, 10
  masters and SmartArt. Rejected because `ppt/authors.xml` and `changesInfo1.xml` hold 11
  individual staff email addresses.
- **UKHSA MenACWY slide set** (2024) and **UKHSA "Vector-borne disease"**: individual staff emails
  in `authors.xml` and `changesInfo1.xml`. Neither has charts.
- **Ofqual "VQ and HE entry"** (2016): staff email addresses on a slide.
- **GCA Annual Review 2019 deck**: an individual email address in `changesInfo1.xml`.
- **WOW Helpdesk Query 32 and 57** (FCDO): a private gmail address and a consultant's
  email/phone on slides.
- **RFG South West meeting notes** (MMO/Defra): 7 email addresses of named individuals.

Rejected for licence doubt (made by a third party; GOV.UK's OGL footer probably does not reach them):

- **GCA Annual Sector Survey 2016** (YouGov for the Groceries Code Adjudicator, pptx). **This is
  the best chart deck found:** 31 native charts (22 bar, 5 pie, 3 doughnut, 1 line), each with an
  embedded workbook, 43 layouts and 0 email/phone hits. But `Company=YouGov Plc`, so authorship
  is not Crown and the licence is not clearly OGL.
- **Promoting gender-smart manufacturing** (WOW Helpdesk/Manufacturing Africa, pptx). It has a
  native bar chart, a legacy MSGraph OLE chart, think-cell tags, VML and SVG, but it is
  contractor-made and uses third-party imagery.

Weaker on features:

- DOCX: Ofsted Board minutes Nov 2017 (no merges), Ofsted Audit Committee minutes March 2017
  (no tables), HMRC ABAB minutes 2024 (no tables, headers or footers), and the CSPL Annual
  Report 2020-21 (no tables, no styles; looks converted from PDF). The IPA Annual Report
  2022-23 has no `docProps/app.xml` and only 12 styles, which suggests it was produced by a
  converter rather than Word.
- XLSX: TPIF Finance Spreadsheet Round 5 (Forestry Commission). It is the runner-up for
  currency: 89 custom formats including £ accounting, 7 Excel tables, 15 conditional-format
  blocks, XLOOKUP/SUMIFS and hidden sheets, but no chart, and it is a form with a role mailbox.
  Also: Parole Board Performance Data (29 formulas, small) and the ESFA suggested
  management-accounts layout (0 formulas).
- PPTX: Health and wellbeing board slide set (PHE, 2015; SmartArt, no chart), OTS webinar
  2021, Defence Cyber Protection Partnership board slides (2018), GCA annual report 2014 and
  2016 decks. None of them has a native chart.
