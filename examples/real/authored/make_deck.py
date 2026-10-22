"""Writes the content of the authored Milleleaf deck (python-pptx 1.0.2, on the box).

    python make_deck.py OUT.pptx MARK.png CHART.jpg

The content is ours: our own text and numbers, the Milleleaf mark (rendered from
brand/milleleaf-mark.svg, unmodified) and a chart picture we made. The test fixtures are what real
apps make of it (LibreOffice Impress, DeckCraft, PowerPoint); this file only writes the content.
It covers what the deck tests check: placeholders that take their layout's position, two-level
bullets, filled and outlined boxes with centred text, tables (one with merged cells and a styled
header row), connectors attached to the shapes they join, web, mail and slide-jump links, speaker
notes on every slide, pictures (one shown twice, one cropped), a shape without text, native bar,
line, pie and combo charts, and a master with several layouts.
"""
import copy
import sys

from lxml import etree
from pptx import Presentation
from pptx.chart.data import CategoryChartData
from pptx.dml.color import RGBColor
from pptx.enum.chart import XL_CHART_TYPE, XL_LEGEND_POSITION
from pptx.enum.dml import MSO_LINE_DASH_STYLE
from pptx.enum.shapes import MSO_CONNECTOR, MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.opc.constants import RELATIONSHIP_TYPE as RT
from pptx.oxml.ns import qn
from pptx.util import Emu, Inches, Pt

out, mark, chart_pic = sys.argv[1], sys.argv[2], sys.argv[3]

TEAL, GREEN, MINT, INK = RGBColor(0x09, 0xC8, 0xBA), RGBColor(0x48, 0x95, 0x57), RGBColor(0x97, 0xDB, 0xC2), RGBColor(0x0B, 0x13, 0x26)

prs = Presentation()  # python-pptx's default template: one master, 11 layouts
prs.slide_width, prs.slide_height = Inches(13.333), Inches(7.5)
L = {l.name: l for l in prs.slide_layouts}
slides = []


def slide(layout, title, notes):
    s = prs.slides.add_slide(L[layout])
    if s.shapes.title is not None and title:
        s.shapes.title.text = title
    s.notes_slide.notes_text_frame.text = notes
    slides.append(s)
    return s


def body(s):
    return next(p for p in s.placeholders if p.placeholder_format.idx == 1)


def link_to_slide(run, target):
    """A text link that jumps to another slide (PowerPoint's "Place in This Document")."""
    rid = run.part.relate_to(target.part, RT.SLIDE)
    rpr = run._r.get_or_add_rPr()
    h = etree.SubElement(rpr, qn("a:hlinkClick"))
    h.set(qn("r:id"), rid)
    h.set("action", "ppaction://hlinksldjump")


# 1. Title slide: the subtitle is what the PowerPoint edit changes (" (revised)").
s1 = slide("Title Slide", "Milleleaf", "Open with what Milleleaf is for: office files that keep their history, whichever app edits them.")
s1.placeholders[1].text = "Office files you can trust, from any app"
s1.shapes.add_picture(mark, Inches(11.2), Inches(0.4), height=Inches(1.6))
band = s1.shapes.add_shape(MSO_SHAPE.RECTANGLE, 0, Inches(7.1), Inches(13.333), Inches(0.4))  # a shape without text
band.fill.solid()
band.fill.fore_color.rgb = TEAL
band.line.fill.background()

# 2. Title and content: two-level bullets and a web link.
s2 = slide("Title and Content", "What Milleleaf does", "Walk through the three promises; the second-level bullets are the detail.")
tf = body(s2).text_frame
items = [
    (0, "Keeps every edit as a signed layer"),
    (1, "Who changed what, and when, stays with the file"),
    (0, "Reads Word, Excel and PowerPoint files from any app"),
    (1, "Re-saves that change nothing come back as no change"),
    (0, "Says what it can't carry, instead of losing it quietly"),
]
tf.text = items[0][1]
for level, text in items[1:]:
    p = tf.add_paragraph()
    p.text, p.level = text, level
p = tf.add_paragraph()
r = p.add_run()
r.text = "milleleaf.com"
r.hyperlink.address = "https://milleleaf.com"

# 3. Two content: a table with merged cells and a styled header row, and a note beside it.
s3 = slide("Two Content", "What comes back from each app", "The table is the point: merged cells span the app columns.")
ph_left = next(p for p in s3.placeholders if p.placeholder_format.idx == 1)
ph_right = next(p for p in s3.placeholders if p.placeholder_format.idx == 2)
x, y, w = ph_left.left, ph_left.top, ph_left.width
ph_left._element.getparent().remove(ph_left._element)
t = s3.shapes.add_table(4, 3, x, y, w, Inches(2.4)).table
rows = [["Format", "Saved by", ""], ["", "LibreOffice", "PowerPoint"], ["Slides", "no change", "no change"], ["Charts", "listed", "listed"]]
for ri, row in enumerate(rows):
    for ci, text in enumerate(row):
        t.cell(ri, ci).text = text
t.cell(0, 1).merge(t.cell(0, 2))  # "Saved by" spans both apps
t.cell(0, 0).merge(t.cell(1, 0))  # "Format" spans two header rows
t.first_row = True
ph_right.text_frame.text = "Re-saves are compared with what Milleleaf would export."

# 4. Title only: a flow of filled, outlined boxes joined by connectors.
s4 = slide("Title Only", "How an edit comes back", "Each arrow is a connector attached to the boxes it joins.")
boxes = []
for i, text in enumerate(["Adopt", "Export", "Edit anywhere", "Import"]):
    b = s4.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(0.8 + i * 3.1), Inches(3.0), Inches(2.4), Inches(1.2))
    b.fill.solid()
    b.fill.fore_color.rgb = MINT
    b.line.color.rgb = TEAL
    b.line.width = Pt(2)
    b.text_frame.text = text
    b.text_frame.paragraphs[0].alignment = PP_ALIGN.CENTER
    b.text_frame.vertical_anchor = MSO_ANCHOR.MIDDLE
    b.text_frame.paragraphs[0].runs[0].font.color.rgb = INK
    boxes.append(b)
for a, b in zip(boxes, boxes[1:]):
    c = s4.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, 0, 0, 0, 0)
    c.begin_connect(a, 3)
    c.end_connect(b, 1)
    c.line.color.rgb = INK
    c.line.width = Pt(1.5)
    c.line._get_or_add_ln().append(etree.fromstring('<a:tailEnd xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" type="triangle"/>'))
under = s4.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(0.8), Inches(4.8), Inches(12.0), Inches(4.8))
under.line.color.rgb = GREEN
under.line.width = Pt(1.5)
under.line.dash_style = MSO_LINE_DASH_STYLE.DASH  # attached to nothing

# 5 and 6. Native charts: bar and line, then pie and a combo (bars with a line).
data = CategoryChartData()
data.categories = ["Mon", "Tue", "Wed", "Thu", "Fri"]
data.add_series("Round trips", (12, 18, 15, 22, 27))
data.add_series("Losses found", (5, 3, 4, 2, 1))
s5 = slide("Title Only", "A week of round trips", "Bars count round trips; the line shows losses falling as they were fixed.")
bar = s5.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(0.5), Inches(1.6), Inches(6.0), Inches(4.8), data).chart
bar.has_legend, bar.legend.position, bar.legend.include_in_layout = True, XL_LEGEND_POSITION.BOTTOM, False
bar.chart_title.text_frame.text = "Round trips and losses"
line = s5.shapes.add_chart(XL_CHART_TYPE.LINE_MARKERS, Inches(6.8), Inches(1.6), Inches(6.0), Inches(4.8), data).chart
line.has_legend = True
line.chart_title.text_frame.text = "The same week, as lines"

pie_data = CategoryChartData()
pie_data.categories = ["Word", "Excel", "PowerPoint"]
pie_data.add_series("Files checked", (40, 35, 25))
s6 = slide("Title Only", "Where the files come from", "The pie is the mix of formats; the combo puts both measures on one chart.")
pie = s6.shapes.add_chart(XL_CHART_TYPE.PIE, Inches(0.5), Inches(1.6), Inches(5.6), Inches(4.8), pie_data).chart
pie.has_legend, pie.plots[0].has_data_labels = True, True
combo_frame = s6.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(6.6), Inches(1.6), Inches(6.2), Inches(4.8), data)
combo = combo_frame.chart
combo.has_legend = True
combo.chart_title.text_frame.text = "Round trips (bars) and losses (line)"
# Make it a combo: move the second series into a line chart on the same axes.
plot_area = combo._chartSpace.find(".//" + qn("c:plotArea"))
bar_chart = plot_area.find(qn("c:barChart"))
second = bar_chart.findall(qn("c:ser"))[1]
bar_chart.remove(second)
line_chart = etree.SubElement(plot_area, qn("c:lineChart"))
etree.SubElement(line_chart, qn("c:grouping")).set("val", "standard")
etree.SubElement(line_chart, qn("c:varyColors")).set("val", "0")
line_chart.append(second)
etree.SubElement(line_chart, qn("c:marker")).set("val", "1")
for ax in bar_chart.findall(qn("c:axId")):
    etree.SubElement(line_chart, qn("c:axId")).set("val", ax.get("val"))
plot_area.remove(line_chart)
bar_chart.addnext(line_chart)

# 7. Picture with caption: our chart picture, cropped.
s7 = slide("Picture with Caption", "The chart, as a picture", "This picture is cropped on purpose: the crop must survive.")
pic_ph = next(p for p in s7.placeholders if p.placeholder_format.type == 18)  # picture placeholder
pic = pic_ph.insert_picture(chart_pic)
pic.crop_left, pic.crop_right, pic.crop_top, pic.crop_bottom = 0.08, 0.05, 0.1, 0.04
next(p for p in s7.placeholders if p.placeholder_format.idx == 2).text = "Made from slide 5's chart."

# 8. Comparison: a plain table.
s8 = slide("Comparison", "Before and after", "A plain table, no merges, for comparison with slide 3.")
for p in list(s8.placeholders):
    if p.placeholder_format.idx in (2, 4):
        p._element.getparent().remove(p._element)
t2 = s8.shapes.add_table(3, 2, Inches(0.8), Inches(2.6), Inches(11.5), Inches(1.6)).table
for ri, row in enumerate([["Before", "After"], ["Losses went unnoticed", "Every loss is listed"], ["One app's view", "Any app's re-save"]]):
    for ci, text in enumerate(row):
        t2.cell(ri, ci).text = text
next(p for p in s8.placeholders if p.placeholder_format.idx == 1).text = "Before"
next(p for p in s8.placeholders if p.placeholder_format.idx == 3).text = "After"

# 9. Section header: a mail link and a link back to the first slide; the mark again.
s9 = slide("Section Header", "Talk to us", "End on the contact address and a way back to the start.")
tf = s9.placeholders[1].text_frame
tf.text = ""
r = tf.paragraphs[0].add_run()
r.text = "hello@milleleaf.com"
r.hyperlink.address = "mailto:hello@milleleaf.com"
p = tf.add_paragraph()
r = p.add_run()
r.text = "Back to the start"
link_to_slide(r, s1)
s9.shapes.add_picture(mark, Inches(11.2), Inches(0.4), height=Inches(1.6))

prs.core_properties.title = "Milleleaf"
prs.core_properties.author = "Milleleaf"
prs.core_properties.last_modified_by = "Milleleaf"  # the default template names its own author
prs.save(out)
print(out, len(prs.slides), "slides")
