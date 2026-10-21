#!/usr/bin/env python3
"""Generates the DOCX / PPTX / XLSX / CSV / TXT / MD fixtures of fixtures/ingest (stdlib only).
Outputs are committed; rerun only when the fixtures change. usage: make_office.py <out-dir>"""
import sys, zipfile, pathlib
from xml.sax.saxutils import escape

out = pathlib.Path(sys.argv[1])
CT_HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'


def zipw(path, files):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in files.items():
            z.writestr(name, data)


# ---------------- DOCX ----------------
W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'


def p(text, style=None, num=False):
    ppr = ""
    if style:
        ppr += f'<w:pStyle w:val="{style}"/>'
    if num:
        ppr += '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>'
    ppr = f"<w:pPr>{ppr}</w:pPr>" if ppr else ""
    return f"<w:p>{ppr}<w:r><w:t xml:space=\"preserve\">{escape(text)}</w:t></w:r></w:p>"


def tbl(rows):
    out_ = "<w:tbl>"
    for r in rows:
        out_ += "<w:tr>" + "".join(f"<w:tc><w:p><w:r><w:t>{escape(c)}</w:t></w:r></w:p></w:tc>" for c in r) + "</w:tr>"
    return out_ + "</w:tbl>"


body = (
    p("Протокол встречи", "Title")
    + p("Обзор проекта", "Heading1")
    + p("Участники обсудили план запуска продукта и ключевые риски.")
    + p("Project overview", "Heading2")
    + p("The team reviewed the launch plan; the codeword is квазар-docx.")
    + p("Первый пункт списка", num=True)
    + p("Second bullet item", num=True)
    + tbl([["Задача", "Owner", "Срок"], ["Подготовить договор", "Анна", "2026-11-01"], ["Review pricing", "Bob", "2026-11-05"]])
    + p("Ссылка на сноску.")
)
docx = {
    "[Content_Types].xml": CT_HEAD + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": CT_HEAD + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": CT_HEAD + f'<w:document {W}><w:body>{body}</w:body></w:document>',
    "word/footnotes.xml": CT_HEAD + f'<w:footnotes {W}><w:footnote w:id="0"><w:p><w:r><w:t>sep</w:t></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>Сноска: источник данных - внутренний отчёт.</w:t></w:r></w:p></w:footnote></w:footnotes>',
    "word/styles.xml": CT_HEAD + f'<w:styles {W}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style></w:styles>',
}
# add a footnote reference to the last paragraph
docx["word/document.xml"] = docx["word/document.xml"].replace(
    "Ссылка на сноску.</w:t></w:r>", 'Ссылка на сноску.</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r>'
)
zipw(out / "sample.docx", docx)

# ---------------- PPTX ----------------
A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'


def slide(title, bullets, table=None):
    sp = f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{escape(title)}</a:t></a:r></a:p></p:txBody></p:sp>'
    paras = "".join(f"<a:p><a:r><a:t>{escape(b)}</a:t></a:r></a:p>" for b in bullets)
    sp += f'<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:txBody>{paras}</p:txBody></p:sp>'
    if table:
        rows = "".join("<a:tr>" + "".join(f"<a:tc><a:txBody><a:p><a:r><a:t>{escape(c)}</a:t></a:r></a:p></a:txBody></a:tc>" for c in r) + "</a:tr>" for r in table)
        sp += f'<p:graphicFrame><a:graphic><a:graphicData><a:tbl>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>'
    return CT_HEAD + f'<p:sld {A}><p:cSld><p:spTree>{sp}</p:spTree></p:cSld></p:sld>'


def notes(text):
    return CT_HEAD + f'<p:notes {A}><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="n"/><p:cNvSpPr/><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>7</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{escape(text)}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>'


def rels(items):
    return CT_HEAD + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + "".join(
        f'<Relationship Id="{i}" Type="{t}" Target="{tg}"/>' for i, t, tg in items) + "</Relationships>"


RT = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
pptx = {
    "[Content_Types].xml": CT_HEAD + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>',
    "_rels/.rels": rels([("rId1", RT + "officeDocument", "ppt/presentation.xml")]),
    "ppt/presentation.xml": CT_HEAD + f'<p:presentation {A}><p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId1"/></p:sldIdLst></p:presentation>',
    # slide order is defined by presentation.xml (rId2 first), not by file names
    "ppt/_rels/presentation.xml.rels": rels([("rId1", RT + "slide", "slides/slide2.xml"), ("rId2", RT + "slide", "slides/slide1.xml")]),
    "ppt/slides/slide1.xml": slide("Стратегия запуска", ["Цель: выйти на рынок в первом квартале", "Key metric: weekly active users"]),
    "ppt/slides/slide2.xml": slide("Roadmap", ["Phase one: beta", "Фаза два: релиз, кодовое слово квазар-pptx"], [["Этап", "Дата"], ["Beta", "2026-12-01"]]),
    "ppt/slides/_rels/slide1.xml.rels": rels([("rId1", RT + "notesSlide", "../notesSlides/notesSlide1.xml")]),
    "ppt/notesSlides/notesSlide1.xml": notes("Заметки докладчика: не забыть показать демо."),
}
zipw(out / "sample.pptx", pptx)

# ---------------- XLSX ----------------
S = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
strings = ["Товар", "Цена", "Дата", "Item", "Price", "Яблоки", "Pears"]
sst = CT_HEAD + f'<sst {S} count="{len(strings)}" uniqueCount="{len(strings)}">' + "".join(f"<si><t>{escape(s)}</t></si>" for s in strings) + "</sst>"
styles = CT_HEAD + f'<styleSheet {S}><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy\\-mm\\-dd"/></numFmts><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164" applyNumberFormat="1"/></cellXfs></styleSheet>'
sheet1 = CT_HEAD + f'<worksheet {S}><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c></row><row r="2"><c r="A2" t="s"><v>5</v></c><c r="B2"><v>120.5</v></c><c r="C2" s="1"><v>46327</v></c></row><row r="3"><c r="A3" t="s"><v>6</v></c><c r="B3"><v>99</v></c><c r="C3" s="1"><v>46328</v></c></row></sheetData></worksheet>'
rows = "".join(f'<row r="{i}"><c r="A{i}"><v>{i}</v></c><c r="B{i}" t="s"><v>3</v></c></row>' for i in range(1, 6))
sheet2 = CT_HEAD + f'<worksheet {S}><sheetData>{rows}</sheetData></worksheet>'
xlsx = {
    "[Content_Types].xml": CT_HEAD + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>',
    "_rels/.rels": rels([("rId1", RT + "officeDocument", "xl/workbook.xml")]),
    "xl/workbook.xml": CT_HEAD + f'<workbook {S}><sheets><sheet name="Прайс" sheetId="1" r:id="rId1"/><sheet name="Data" sheetId="2" r:id="rId2"/></sheets></workbook>',
    "xl/_rels/workbook.xml.rels": rels([("rId1", RT + "worksheet", "worksheets/sheet1.xml"), ("rId2", RT + "worksheet", "worksheets/sheet2.xml"), ("rId3", RT + "sharedStrings", "sharedStrings.xml"), ("rId4", RT + "styles", "styles.xml")]),
    "xl/sharedStrings.xml": sst,
    "xl/styles.xml": styles,
    "xl/worksheets/sheet1.xml": sheet1,
    "xl/worksheets/sheet2.xml": sheet2,
}
zipw(out / "sample.xlsx", xlsx)

# ---------------- plain text ----------------
(out / "sample.csv").write_text("Имя;Город;Сумма\nИван;Москва;1 200,50\nMary;London;\"3,400\"\n\"Пётр \"\"Лёха\"\"\";Казань;7\n", encoding="utf-8")
(out / "sample.tsv").write_text("name\tcity\nAnna\tРига\n", encoding="utf-8")
(out / "sample.txt").write_text("Заметки по проекту.\nSecond line in English: codeword квазар-txt.\n", encoding="utf-8")
(out / "sample-cp1251.txt").write_bytes("Привет, мир! Это тестовый файл в кодировке windows-1251 про договор поставки.\nВторая строка.\n".encode("cp1251"))
(out / "sample.md").write_text("---\ntitle: Заметка\ntags: [a, b]\n---\n# Заголовок\n\nТекст заметки / note text, quasar-md.\n", encoding="utf-8")
# not an office file at all, with an office extension
(out / "broken.docx").write_bytes(b"this is not a zip archive")
(out / "empty.pdf").write_bytes(b"")
(out / "garbage.pdf").write_bytes(b"%PDF-1.4\nnot really a pdf\n%%EOF\n")
