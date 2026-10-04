# Test tool for ra_acro's manual test (acro_live): writes a hand-built 3-page text PDF.
# Usage: py mkpdf.py OUT.pdf [--untagged] [--dense] [--struct]   (no dependencies)
# --dense: page one holds 40 lines (~440 words) at 11 pt, for the hover bench.
# --struct: page one is tagged H1, P, then a list (L > LI > LBody) of two items;
#   every other line stays a P.
# Page 3 holds the marker word ZEBRAFINCH, which the test reads up to and scrolls to.
import sys
PAGES = [
 ["Page one. Nancy Pelosi bought call options before the vote.",
  "Volodymyr Zelensky spoke to the parliament on Tuesday morning.",
  "The quick brown fox jumps over the lazy dog near the river.",
  "Alpha bravo charlie delta echo foxtrot golf hotel india juliet."],
 ["Page two. Markets rallied after the central bank held rates.",
  "Pelosi and Zelensky were both mentioned in the evening briefing.",
  "Seven sailors sailed southward seeking silver and shelter.",
  "Kilo lima mike november oscar papa quebec romeo sierra tango."],
 ["Page three. This is the final page of the probe document.",
  "The marker word here is ZEBRAFINCH for scroll verification.",
  "Uniform victor whiskey xray yankee zulu end of document.",
  "Last line on page three with Pelosi one more time."],
]
def esc(s): return s  # test text has no parens or backslashes
def build(tagged, out, dense=False, struct=False):
    pages = [list(p) for p in PAGES]
    if dense:
        words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo".split()
        pages[0] += [f"Row{i:02} " + ' '.join(words[(i + k) % len(words)] for k in range(10)) for i in range(36)]
    size, gap, top = (11, 16, 740) if dense else (16, 40, 720)
    objs = {}
    # 1 catalog, 2 pages, 3 font, then per page: page obj, content obj
    n_pages = len(pages)
    page_ids = [4+2*i for i in range(n_pages)]
    cont_ids = [5+2*i for i in range(n_pages)]
    nxt = 4+2*n_pages
    struct_root = nxt; doc_elem = nxt+1; parent_tree = nxt+2; nxt += 3
    p_elems = []  # (objid, page_index, mcid): the element holding each line
    tags = {}     # objid -> (struct type, parent objid or None = Document)
    top_kids = [] # Document's children, in order
    lst = None
    for pi, lines in enumerate(pages):
        for li in range(len(lines)):
            kind = "P"
            if struct and pi == 0:
                kind = {0: "H1", 1: "P"}.get(li, "LI")
            if kind == "LI":
                if lst is None:
                    lst = nxt; nxt += 1; tags[lst] = ("L", None); top_kids.append(lst)
                item = nxt; body = nxt + 1; nxt += 2
                tags[item] = ("LI", lst); tags[body] = ("LBody", item)
                p_elems.append((body, pi, li))
            else:
                lst = None
                tags[nxt] = (kind, None); top_kids.append(nxt)
                p_elems.append((nxt, pi, li)); nxt += 1
    cat = "<< /Type /Catalog /Pages 2 0 R"
    if tagged:
        cat += f" /MarkInfo << /Marked true >> /StructTreeRoot {struct_root} 0 R /Lang (en-US)"
    objs[1] = cat + " >>"
    objs[2] = f"<< /Type /Pages /Count {n_pages} /Kids [{' '.join(f'{p} 0 R' for p in page_ids)}] >>"
    objs[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    for pi, lines in enumerate(pages):
        extra = f" /StructParents {pi} /Tabs /S" if tagged else ""
        objs[page_ids[pi]] = (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 3 0 R >> >> /Contents {cont_ids[pi]} 0 R{extra} >>")
        s = ""
        for li, ln in enumerate(lines):
            y = top - li*gap
            body = f"BT /F1 {size} Tf 72 {y} Td ({esc(ln)}) Tj ET\n"
            s += (f"/P << /MCID {li} >> BDC\n{body}EMC\n") if tagged else body
        objs[cont_ids[pi]] = f"<< /Length {len(s.encode('latin-1'))} >>\nstream\n{s}endstream"
    if tagged:
        objs[struct_root] = f"<< /Type /StructTreeRoot /K [{doc_elem} 0 R] /ParentTree {parent_tree} 0 R /ParentTreeNextKey {n_pages} >>"
        objs[doc_elem] = f"<< /Type /StructElem /S /Document /P {struct_root} 0 R /K [{' '.join(f'{o} 0 R' for o in top_kids)}] >>"
        nums = []
        for pi in range(n_pages):
            arr = ' '.join(f'{o} 0 R' for o,p,_ in p_elems if p==pi)
            nums.append(f"{pi} [{arr}]")
        objs[parent_tree] = f"<< /Nums [{' '.join(nums)}] >>"
        holders = {o: (pi, mcid) for o, pi, mcid in p_elems}
        for o, (kind, parent) in tags.items():
            par = parent if parent is not None else doc_elem
            if o in holders:
                pi, mcid = holders[o]
                k = f"/Pg {page_ids[pi]} 0 R /K {mcid}"
            else:
                k = "/K [" + ' '.join(f'{c} 0 R' for c, (_, pp) in tags.items() if pp == o) + "]"
            objs[o] = f"<< /Type /StructElem /S /{kind} /P {par} 0 R {k} >>"
    else:
        for k in [struct_root, doc_elem, parent_tree] + list(tags):
            objs[k] = "null"
    maxid = max(objs)
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"; offs = {}
    for i in range(1, maxid+1):
        offs[i] = len(buf)
        buf += f"{i} 0 obj\n{objs.get(i,'null')}\nendobj\n".encode('latin-1')
    xref = len(buf)
    buf += f"xref\n0 {maxid+1}\n0000000000 65535 f \n".encode()
    for i in range(1, maxid+1): buf += f"{offs[i]:010d} 00000 n \n".encode()
    buf += f"trailer\n<< /Size {maxid+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    open(out,'wb').write(buf)
if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: mkpdf.py OUT.pdf [--untagged] [--dense]")
    a = sys.argv[2:]
    build("--untagged" not in a, sys.argv[1], "--dense" in a, "--struct" in a)
    print("ok")
